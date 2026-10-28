// Local workerd fixture only. Never deploy this module. D1 and R2 are real
// isolated bindings; managed AI Search is a deterministic, no-network fake.
import application from "../../apps/worker/build/worker/shim.mjs";

const items = new Map();
const calls = { upload: 0, info: 0, list: 0, delete: 0 };
let nextStatus = "queued";
let deleteFails = false;
let publication = "automatic";

function testEnv(env) {
  const provider = {
    items: {
      async list({ key, source }) {
        calls.list++;
        return { result: [...items.values()].filter(item => item.key === key && item.source_id === source) };
      },
      async upload(key) {
        calls.upload++;
        const item = { id: crypto.randomUUID(), key, source_id: "builtin", status: nextStatus };
        items.set(key, item);
        return item;
      },
      get(id) {
        return { async info() {
          calls.info++;
          const item = [...items.values()].find(item => item.id === id);
          if (!item) throw new Error("item_not_found");
          return item;
        } };
      },
      async delete(id) {
        calls.delete++;
        if (deleteFails) throw new Error("simulated provider outage");
        for (const [key, item] of items) if (item.id === id) items.delete(key);
      },
    },
  };
  return {
    ...env, SEMANTIC_SEARCH: "on", R2_LIST_PAGE_MAX: "1",
    BRAINS_CLOUD_CONFIG: JSON.stringify({ semantic: { instance: "local-cost-fixture", publication } }),
    AI_SEARCH: { get: () => provider },
  };
}

async function isAdmin(request, env) {
  const token = request.headers.get("Authorization")?.replace(/^Bearer /, "") ?? "";
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
  const digest = [...new Uint8Array(bytes)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  return JSON.parse(env.TOKEN_REGISTRY_JSON).tokens.some(token =>
    token.enabled && token.token_sha256 === digest && token.scopes.includes("admin:/"));
}

export default {
  async fetch(request, original, ctx) {
    const env = testEnv(original);
    const path = new URL(request.url).pathname;
    if (!path.startsWith("/_cost/")) {
      const background = [];
      const instance = new application({
        waitUntil(work) { background.push(work); ctx.waitUntil(work); },
      }, env);
      const response = await instance.fetch(request);
      await Promise.all(background);
      return response;
    }
    if (!await isAdmin(request, env)) return new Response("unauthorized", { status: 401 });
    if (path === "/_cost/seed-scan") {
      const { start, count } = await request.json();
      if (count > 50) throw new Error("fixture seed batch exceeds 50 objects");
      await Promise.all(Array.from({ length: count }, (_, offset) =>
        env.BRAINS_BUCKET.put(`vaults/${env.VAULT_ID}/heads/cost-scan/${String(start + offset).padStart(5, "0")}.md.json`, "{}")));
    } else if (path === "/_cost/settings-offline") {
      await env.BRAINS_INDEX.exec("ALTER TABLE resource_settings RENAME TO resource_settings_offline");
    } else if (path === "/_cost/settings-online") {
      await env.BRAINS_INDEX.exec("ALTER TABLE resource_settings_offline RENAME TO resource_settings");
    } else if (path === "/_cost/publication") {
      publication = (await request.json()).mode;
    } else if (path === "/_cost/drop-provider-item") {
      const { revision } = await request.json();
      items.delete(`p1/${revision}.md`);
    } else if (path === "/_cost/invalidate-live") {
      const { path: source } = await request.json();
      await env.BRAINS_INDEX.prepare("UPDATE semantic_items SET state='deleting',updated_at=0 WHERE path=?1 AND state='ready'").bind(source).run();
    } else if (path === "/_cost/provider") {
      const config = await request.json();
      if (config.status) nextStatus = config.status;
      if (config.existingStatus) for (const item of items.values()) item.status = config.existingStatus;
      if (config.deleteFails !== undefined) deleteFails = config.deleteFails;
    } else if (path === "/_cost/arm" || path === "/_cost/tick") {
      await env.BRAINS_INDEX.prepare("UPDATE semantic_items SET updated_at=0 WHERE state IN ('pending','queued','deleting')").run();
      if (path === "/_cost/tick") await new application(ctx, env).scheduled({ scheduledTime: Date.now(), cron: "*/5 * * * *" });
    }
    const jobs = await env.BRAINS_INDEX.prepare("SELECT path,revision,state,last_error FROM semantic_items ORDER BY path,revision").all();
    const usage = await env.BRAINS_INDEX.prepare("SELECT rule,SUM(consumed) AS consumed,SUM(reserved) AS reserved FROM resource_usage GROUP BY rule ORDER BY rule").all();
    const plans = [];
    if (path === "/_cost/plans") for (const states of ["'pending','queued','deleting'", "'failed','blocked','delete_failed'"]) {
      const result = await env.BRAINS_INDEX.prepare(`EXPLAIN QUERY PLAN SELECT revision FROM semantic_items WHERE space=?1 AND state IN (${states}) AND updated_at<unixepoch()-30 ORDER BY updated_at,revision LIMIT 1`).bind("fixture").all();
      plans.push(...result.results.map(row => row.detail));
    }
    return Response.json({ calls, jobs: jobs.results, usage: usage.results, items: [...items.values()], plans });
  },
};
