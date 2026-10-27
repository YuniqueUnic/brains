# Read-only MCP on Cloudflare

`apps/mcp` is a separate Worker using the Cloudflare Agents SDK's stateless MCP handler and the official MCP SDK. It exposes three tools: `search`, `list_notes` and `read_note`. Each tool calls the protected Brains API through a service binding. The adapter has no file mutation tool and accepts no caller-supplied upstream URL or credential.

## Credentials and deployment

The MCP ingress credential and upstream Brains credential are separate from the vault owner's token. `cloudflare:mcp-init` adds an enabled `mcp-reader` entry containing only read scopes to the private registry; it retains all existing entries and signing keys. Set `BRAINS_MCP_READ_PREFIX` before initialization to narrow the accessible subtree.

```dotenv
BRAINS_MCP_READ_PREFIX=/reference
BRAINS_MCP_WORKER_NAME=brains-mcp
BRAINS_MCP_HOSTNAME=brains-tools.example.com
```

```sh
pnpm cloudflare:mcp-init
# Publish the updated token registry to the existing vault Worker first.
pnpm cloudflare:deploy
pnpm cloudflare:mcp-check
pnpm cloudflare:mcp-deploy
pnpm cloudflare:mcp-verify
```

Initialization writes `BRAINS_MCP_API_TOKEN` and `BRAINS_MCP_ACCESS_TOKEN` to the private `.env` with mode 0600. Do not copy these values into a repository, issue, chat or command argument. The deployment helper verifies that the upstream credential matches an enabled registry entry with read scopes only, then uploads the two MCP secrets through a private temporary file. It binds `BRAINS_API` to `BRAINS_WORKER_NAME`; the adapter's name defaults to `<BRAINS_WORKER_NAME>-mcp`.

A custom hostname must belong to the selected Cloudflare account. Without one, Wrangler uses the Worker's configured default ingress. The endpoint is `/mcp`. Missing or incorrect ingress credentials return 401; a missing upstream credential returns 503. Tool execution remains subject to Brains scope, revision, size and resource policies. A narrow reader must pass its explicit prefix to search/list rather than asking for the entire vault.

## Owner-only Access portal

Create the portal only after its upstream MCP Worker is reachable. Cloudflare's portal is a managed outer layer; it does not replace the adapter or Brains authorization. Start with one owner and the three read-only tools. [Official portal setup](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/).

1. In Zero Trust, register the adapter's HTTPS `/mcp` endpoint as an MCP server. Use bearer authentication with `BRAINS_MCP_ACCESS_TOKEN`, not the vault admin token. The upstream uses the same dedicated reader for this owner-only configuration.
2. Create a portal on a separate hostname, attach that server and enable only `search`, `list_notes` and `read_note`. Keep Code Mode off until it is needed.
3. Inspect the automatically created Access application. Replace broad access with an Allow policy matching the owner's exact email and the selected identity provider. Confirm that no Everyone, bypass or unintended service-token policy grants access.
4. Verify portal DNS and TLS. API/Terraform creation requires an explicit CNAME to `gateway.agents.cloudflare.com`; the dashboard normally creates it. Do not replace an existing record or Worker route.
5. Authenticate as the owner and call all three tools. Confirm that an unauthenticated client is challenged, a different identity is denied, paths outside the upstream read scope fail, and no mutation tool exists.

Portal OAuth and upstream bearer authentication have different owners. For a future multi-user portal, revisit per-user upstream authorization instead of silently sharing this reader across teammates. Portal capacity, Access seats and log retention are catalogued in the resource profile; they are platform-managed values, not counters inferred from MCP requests.

## Verification boundaries

`pnpm cloudflare:mcp-verify` uses the official client SDK for protocol negotiation, discovers the three tools, performs a scoped list/search and checks unauthenticated ingress. It needs the configured MCP hostname and private token. It does not publish, mutate files or configure a portal.

`pnpm mcp:check` regenerates platform/binding types from Wrangler and checks the SDK contract. `pnpm --filter brains-mcp build` bundles the adapter without deployment. Runtime validation must cover initialize, tool discovery, read/search/list calls, wrong credentials and out-of-scope reads. Actual Access login and owner policies require a separately configured portal and identity provider; a successful direct MCP call does not prove that policy.

Raw note content is returned as data with its source path and revision. The adapter refuses binary notes and text above 256 KiB. It adds no note/query content or credentials to custom traces. Cloudflare platform/portal telemetry has its own retention and permissions; configure it separately from AI Gateway inference logs.
