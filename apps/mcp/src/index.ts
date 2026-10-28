import { McpServer } from "@modelcontextprotocol/server";
import { createMcpHandler } from "agents/mcp/server";
import { z } from "zod";
import manifest from "../package.json";

async function authorized(request: Request, expected: string): Promise<boolean> {
  const token = request.headers.get("Authorization");
  if (!expected || !token?.startsWith("Bearer ") || token.length === 7) return false;
  const encoder=new TextEncoder();
  const key=await crypto.subtle.importKey("raw",encoder.encode(expected),{name:"HMAC",hash:"SHA-256"},false,["sign","verify"]);
  const message=encoder.encode("brains-mcp-access");
  const provided=await crypto.subtle.importKey("raw",encoder.encode(token.slice(7)),{name:"HMAC",hash:"SHA-256"},false,["sign"]);
  return crypto.subtle.verify("HMAC",key,await crypto.subtle.sign("HMAC",provided,message),message);
}

async function api(env: Env, path: string, body?: unknown) {
  const response = await env.BRAINS_API.fetch(new Request(`https://brains.internal/api/v1${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { Authorization: `Bearer ${env.BRAINS_API_TOKEN}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  }));
  if (!response.ok) {
    const error = await response.json() as { error?: { message?: string } };
    throw new Error(error.error?.message ?? `Brains returned HTTP ${response.status}`);
  }
  return response;
}

function result(value: unknown) {
  return { content: [{ type: "text" as const, text: JSON.stringify(value) }] };
}

function server(env: Env) {
  const mcp = new McpServer({ name: "brains", version: manifest.version });
  mcp.registerTool("search", {
    description: "Search authorized current notes; return source paths and revisions.",
    inputSchema: z.object({ query: z.string().min(1).max(512), prefix: z.string().optional(), tags: z.array(z.string()).optional() }),
    annotations: { readOnlyHint: true },
  }, async ({ query, prefix, tags }) => result(await (await api(env, "/search", { q: query, mode: "hybrid", prefix, tags })).json()));
  mcp.registerTool("list_notes", {
    description: "List live notes within the configured read scope.",
    inputSchema: z.object({ prefix: z.string().optional(), cursor: z.string().optional(), limit: z.number().int().min(1).max(100).default(50) }),
    annotations: { readOnlyHint: true },
  }, async ({ prefix, cursor, limit }) => result(await (await api(env, "/files/query", { prefix, cursor, limit, source: "store" })).json()));
  mcp.registerTool("read_note", {
    description: "Read a current text note with its source revision. Binary and oversized files require the file API.",
    inputSchema: z.object({ path: z.string().min(1) }),
    annotations: { readOnlyHint: true },
  }, async ({ path }) => {
    const response = await api(env, `/file?path=${encodeURIComponent(path)}`);
    const media = response.headers.get("X-Brain-Media-Type") ?? response.headers.get("Content-Type") ?? "";
    if (!/^(text\/|application\/(json|yaml|toml))/.test(media)) throw new Error("This file is not a text note.");
    const maximum = 262144;
    if (Number(response.headers.get("Content-Length")) > maximum) { await response.body?.cancel(); throw new Error("This note exceeds the MCP text limit; use the file API."); }
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.length > maximum) throw new Error("This note exceeds the MCP text limit; use the file API.");
    return result({ path, revision: response.headers.get("ETag")?.replaceAll('"', ""), text: new TextDecoder("utf-8", { fatal: true }).decode(bytes) });
  });
  return mcp;
}

export default {
  async fetch(request: Request, env: Env, ctx: Parameters<ReturnType<typeof createMcpHandler>>[2]) {
    if (new URL(request.url).pathname !== "/mcp") return new Response("Not found", { status: 404 });
    if (!await authorized(request, env.MCP_ACCESS_TOKEN)) {
      return new Response("Unauthorized", { status: 401, headers: { "WWW-Authenticate": "Bearer" } });
    }
    if (!env.BRAINS_API_TOKEN) return new Response("Upstream read credential is not configured", { status: 503 });
    return createMcpHandler(() => server(env))(request, env, ctx);
  },
};
