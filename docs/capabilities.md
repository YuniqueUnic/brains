# Capabilities and how they are controlled

What each runtime can do, where a capability comes from, and what can be
switched off at run time. Read this before wiring an agent to an instance:
`GET /api/v1/capabilities` reports the same matrix for the connected
instance, and `bsx capabilities --json` prints it for any profile.

## The matrix

| Capability | Standalone `bsx` (local profile) | `brains-server` (native, release build) | Docker image | Worker (`cloudflare-rust`) |
|---|---|---|---|---|
| Literal search | yes | yes | yes | yes |
| Full-text search (FTS) | yes (opt-in Tantivy) | yes (Turso FTS5) | yes | yes (D1 FTS5) |
| Hybrid ranked search | yes | yes | yes | yes, degraded: semantic explicitly unavailable |
| Semantic search | declared unavailable (no embedding provider yet) | declared unavailable | declared unavailable | ready for the AI binding; see the roadmap issue |
| Document conversion (`bsx convert`) | yes | declared (conversion runs client-side) | declared | declared |
| URL import (`bsx import-url`) | yes | n/a (client-side) | n/a | n/a |
| `bsx check`, `bsx checkpoint` | yes | yes (against a remote profile too) | yes | yes |
| Working copy (`checkout/status/pull/push`) | yes | yes | yes | yes |
| Archive export/import, sync | yes | yes | yes | yes |
| Lifecycle/trust metadata filters | yes | yes | yes | yes |

"Declared" capabilities on servers are an advertisement: the web app and
agents read them to decide what to offer. `bsx convert` and `bsx import-url`
always run in the CLI against existing APIs, so the server's declaration is
the only thing to control.

## Where a capability comes from

Three layers, in order of authority:

1. **Compile-time features.** `brains-cli` defaults to `fts`, `convert`,
   `url-import`; `brains-server` defaults to `fts`, `convert`. A build with
   `--no-default-features` reports the missing capabilities as absent — it
   never claims something it cannot do. Release archives and Docker images
   ship the default (full) builds.
2. **Boot-time arguments.** `brains-server --disable` (env
   `BRAINS_DISABLE`, comma-separated) stops *advertising* a capability.
   Only two names are accepted, because only those can be withheld honestly:
   - `document_conversion` — the server only advertises conversion; the
     CLI performs it, so not advertising it fully disables the flow.
   - `semantic` — hides the semantic block from hybrid-search clients.
   Any other name (for example `fts`) fails startup: FTS is a compile-time
   store capability and pretending otherwise would be a lie.
3. **Runtime environment (Worker only).** `SEMANTIC_SEARCH=off`, or a
   missing `AI` binding, makes the Worker report semantic search as
   explicitly unavailable, with the reason in `capabilities.semantic.
   unavailable_reason`.

## How state actually changes (and why there is no event bus)

There is deliberately no config-watch or publish/subscribe machinery.
Writes return their outcome — including `index_state` — in the same
response; every reader (the capabilities endpoint, the search `building`
flag, the CLI config file, Worker env) re-derives state at use time; the
only long-lived mutable state is the boot-time token registry. A lost index
degrades loudly (`503` / `index_state: "building"`) and heals through
explicit `bsx reindex --all-pages`, not through background promises. If you
are looking for the "config update event" to subscribe to: it is the HTTP
response of the write, and re-reading is the subscription.

## Checking what an instance really supports

```bash
bsx capabilities --json | jq '.features, .document_conversion, .semantic'
curl -s -H "Authorization: Bearer $TOKEN" "$BASE_URL/api/v1/capabilities" | jq .
```

`build_version` reports the serving instance's version; it can differ from
the CLI's own version.
