import assert from "node:assert/strict";
import { Client, StreamableHTTPClientTransport } from "@modelcontextprotocol/client";

const origin = process.env.BRAINS_MCP_HOSTNAME;
const token = process.env.BRAINS_MCP_ACCESS_TOKEN;
if (!origin || !token) throw new Error("Configure BRAINS_MCP_HOSTNAME and BRAINS_MCP_ACCESS_TOKEN");
const url = new URL(`https://${origin}/mcp`);
const client = new Client({ name: "brains-verification", version: "1" }, { capabilities: {} });
const transport = new StreamableHTTPClientTransport(url, {
  authProvider: { token: async () => token },
  requestInit: { headers: { "User-Agent": "brains-cloudflare-check" } },
});

try {
  await client.connect(transport);
  const { tools } = await client.listTools();
  assert.deepEqual(tools.map((tool) => tool.name).sort(), ["list_notes", "read_note", "search"]);
  const prefix = process.env.BRAINS_MCP_READ_PREFIX || "/";
  for (const [name, args] of [["list_notes", { prefix, limit: 1 }], ["search", { query: "knowledge", prefix }]]) {
    const result = await client.callTool({ name, arguments: args });
    assert.equal(Boolean(result.isError), false, JSON.stringify(result));
  }
  const unauthenticated = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", "User-Agent": "brains-cloudflare-check" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list" }),
  });
  await unauthenticated.body?.cancel();
  assert.equal(unauthenticated.status, 401);
  console.log("Verified MCP SDK negotiation, three read-only tools, scoped listing/search and ingress authentication.");
} finally {
  await client.close();
}
