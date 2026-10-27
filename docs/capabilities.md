# Capabilities and how they are controlled

What each runtime can do, where a capability comes from, and what can be
switched off at run time. Read this before wiring an agent to an instance:
`GET /api/v1/capabilities` reports the same matrix for the connected
instance, and `bsx capabilities --json` prints it for any profile.

## The matrix

| Capability | Standalone `bsx` (local profile) | `brains-server` (native, release build) | Docker image | Worker (`cloudflare-rust`) |
|---|---|---|---|---|
| Literal search | yes | yes | yes | yes |
| Full-text search (FTS) | yes (Tantivy-backed, default on) | yes (Tantivy-backed, default on) | yes | yes (SQLite FTS5 on D1) |
| Hybrid ranked search | yes | yes | yes | yes; semantic joins when configured |
| Semantic search | declared unavailable (no embedding provider yet) | declared unavailable | declared unavailable | optional managed AI Search |
| Document conversion (`bsx convert`) | yes | advertised (conversion runs client-side) | advertised | optional remote conversion; native CLI fallback remains available |
| URL import (`bsx import-url`) | yes | n/a (client-side) | optional remote import | n/a |
| `bsx check`, `bsx checkpoint` | yes | yes (against a remote profile too) | yes | yes |
| Working copy (`checkout/status/pull/push`) | yes | yes | yes | yes |
| Archive export/import, sync | yes | yes | yes | yes |
| Lifecycle/trust metadata filters | yes | yes | yes | yes |

"Declared" capabilities on servers are an advertisement: the web app and
agents read them to decide what to offer. `bsx convert` and `bsx import-url`
prefer a mounted remote provider when the instance advertises it, including in a thin CLI build. Otherwise a full native CLI uses its own converter/fetcher against the file APIs. Provider availability and request permissions are separate checks.

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
      CLI performs it independently, so hiding it does not disable CLI conversion.
   - `semantic` — hides the semantic block from hybrid-search clients.
   `semantic_search` is accepted as an alias of `semantic`. Any other name
   (for example `fts`) fails startup: FTS is a compile-time store capability
   and pretending otherwise would be a lie. Note that disabling only removes
   the advertisement — capability-respecting clients (the web app, agents
   that read `/capabilities`) stop offering the flow.
3. **Runtime providers (Worker).** `BRAINS_CLOUD_CONFIG` installs conversion/URL/semantic providers; the helper adds matching bindings. `SEMANTIC_SEARCH=off` prevents mounting semantic retrieval. Resource policy admission reports quota or unknown-cost restrictions separately from implementation availability. The web app offers conversion/import only when mounted and the session can write; administrator settings can inspect resource usage. See [cloud configuration](cloudflare.md) and [retrieval behavior](cloudflare-ai-plan.md).

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
