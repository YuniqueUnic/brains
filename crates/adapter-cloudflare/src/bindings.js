// @ts-check

import { tracing } from "cloudflare:workers";
import robotsParser from "robots-parser";

/** @typedef {{AI: Ai, AI_SEARCH: AiSearchNamespace}} CloudBindings */

// SDK glue only. Do not attach queries, notes, URLs or credentials to spans.
/** @param {CloudBindings} env @param {string} name @param {Uint8Array<ArrayBuffer>} bytes @param {string} media */
export async function markdown(env, name, bytes, media) {
  return tracing.enterSpan("brains.convert", () => env.AI.toMarkdown({
    name, blob: new Blob([bytes], { type: media }),
  }));
}

/** @param {CloudBindings} env @param {string} name */
function instance(env, name) { return env.AI_SEARCH.get(name); }

/** @param {CloudBindings} env @param {string} name @param {string} query @param {number} limit @param {number} threshold */
export async function searchQuery(env, name, query, limit, threshold) {
  return tracing.enterSpan("brains.semantic.query", () => instance(env, name).search({
    query,
    ai_search_options: {
      query_rewrite: { enabled: false },
      retrieval: { retrieval_type: "vector", max_num_results: limit, match_threshold: threshold, return_on_failure: false },
      reranking: { enabled: false },
      cache: { enabled: false },
    },
  }));
}

/** @param {CloudBindings} env @param {string} name @param {string} key */
export async function findItem(env, name, key) {
  const page = await instance(env, name).items.list({ search: key, per_page: 50 });
  return page.result.find(item => item.key === key) ?? null;
}

/** @param {CloudBindings} env @param {string} name @param {string} key @param {string} body */
export async function uploadItem(env, name, key, body) {
  return tracing.enterSpan("brains.semantic.publish", () => instance(env, name).items.upload(key, body));
}

/** @param {CloudBindings} env @param {string} name @param {string} id */
export async function itemInfo(env, name, id) {
  try { return await instance(env, name).items.get(id).info(); }
  catch (error) { if (error instanceof Error && "status" in error && error.status === 404) return null; throw error; }
}
/** @param {CloudBindings} env @param {string} name @param {string} id */
export async function deleteItem(env, name, id) {
  return tracing.enterSpan("brains.semantic.remove", () => instance(env, name).items.delete(id));
}

/** @param {string} kind @param {string} message @param {number} [size] */
function fetchError(kind, message, size) {
  return Object.assign(new Error(message), { kind, size });
}

/** @param {string} raw */
function publicUrl(raw) {
  let url;
  try { url = new URL(raw); } catch { throw fetchError("invalid_argument", "URL import requires an absolute public HTTPS URL"); }
  if (url.protocol !== "https:" || url.username || url.password || (url.port && url.port !== "443")) {
    throw fetchError("invalid_argument", "URL import requires public HTTPS on the standard port without credentials");
  }
  const host = url.hostname.toLowerCase();
  if (host === "localhost" || host.endsWith(".localhost") || host.endsWith(".local") || host.endsWith(".internal")) {
    throw fetchError("forbidden", "Private URL target is not allowed");
  }
  url.hash = "";
  return url;
}

/** @param {string} raw @param {AbortSignal} signal @param {boolean} [checkRobots] */
async function fetchRedirects(raw, signal, checkRobots = false) {
  let current = publicUrl(raw);
  for (let redirects = 0; redirects <= 3; redirects++) {
    if (checkRobots && !await robotsAllow(current, signal)) throw fetchError("forbidden", "robots.txt disallows this path");
    // global_fetch_strictly_public enforces public DNS/IP destinations.
    const response = await fetch(current, {
      redirect: "manual", signal,
      headers: { Accept: "text/markdown, text/html, application/pdf, text/plain;q=0.9", "User-Agent": "brains-url-import/0.6" },
    });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const next = response.headers.get("Location");
      await response.body?.cancel();
      if (!next) throw new Error("Redirect has no Location");
      current = publicUrl(new URL(next, current).href);
      continue;
    }
    return { response, url: current };
  }
  throw new Error("Too many redirects");
}

/** @param {Response} response @param {number} maximum */
async function boundedBody(response, maximum) {
  const length = Number(response.headers.get("Content-Length"));
  if (Number.isFinite(length) && length > maximum) {
    await response.body?.cancel();
    throw fetchError("too_large", "URL response exceeds the configured byte limit", length);
  }
  if (!response.body) return new Uint8Array();
  const reader = response.body.getReader();
  const parts = [];
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > maximum) { await reader.cancel(); throw fetchError("too_large", "URL response exceeds the configured byte limit", size); }
    parts.push(value);
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const part of parts) { bytes.set(part, offset); offset += part.length; }
  return bytes;
}

/** @param {URL} url @param {AbortSignal} signal */
async function robotsAllow(url, signal) {
  const robots = await fetchRedirects(new URL("/robots.txt", url).href, signal);
  if (robots.response.status === 404 || robots.response.status === 410) { await robots.response.body?.cancel(); return true; }
  if (!robots.response.ok) { await robots.response.body?.cancel(); throw new Error("robots.txt could not be checked"); }
  const body = new TextDecoder().decode(await boundedBody(robots.response, 262144));
  return robotsParser(new URL("/robots.txt", url).href, body).isAllowed(url.href, "brains-url-import") !== false;
}

/** @param {string} raw @param {number} maximum */
export async function fetchPublic(raw, maximum) {
  return tracing.enterSpan("brains.url.fetch", async () => {
    const signal = AbortSignal.timeout(25000);
    const requested = publicUrl(raw);
    const { response, url } = await fetchRedirects(requested.href, signal, true);
    if (!response.ok) { await response.body?.cancel(); throw new Error(`URL fetch returned HTTP ${response.status}`); }
    const bytes = await boundedBody(response, maximum);
    // Query strings may carry credentials; they are not persisted as provenance.
    url.search = "";
    return { url: url.href, media_type: response.headers.get("Content-Type") ?? "text/html", bytes };
  });
}
