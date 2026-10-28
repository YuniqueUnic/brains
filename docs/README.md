# Brains documentation

Documentation for the Brains repository, written against the code as it is
(not as it was drafted). Start here:

| Document | Contents |
|---|---|
| [architecture.md](architecture.md) | How the system works: crates, storage layout, commit protocol, auth, API contract, runtimes |
| [runbook.md](runbook.md) | How to run, configure, back up, deploy, and troubleshoot |
| [ai-integration.md](ai-integration.md) | Agent-facing API reference and write etiquette |
| [editor-search.md](editor-search.md) | Source tags, search commands, and editor width |
| [releasing.md](releasing.md) | Unified versions and release workflows |
| [capabilities.md](capabilities.md) | Capability matrix per runtime, compile features, `BRAINS_DISABLE`, how state changes propagate |
| [cloudflare.md](cloudflare.md) | Existing Worker deployment through shared dotenv/Actions configuration, bootstrap and verification |
| [cloudflare-ai-plan.md](cloudflare-ai-plan.md) | Cloud ingestion, managed semantic retrieval, source validation and operational boundaries |
| [cloudflare-quotas.md](cloudflare-quotas.md) | Composable resource rules, dated Free snapshot, quota admission and measured enforcement |
| [cloudflare-mcp.md](cloudflare-mcp.md) | Dedicated read-only MCP Worker and owner-only Access portal setup |
| [../api/README.md](../api/README.md) | Slash-separated HTTP route conventions and the required CI gate |
| [cloudflare-verification.md](cloudflare-verification.md) | Local and isolated-cloud business verification, reproduction commands and untested boundaries |
| [native-release.md](native-release.md) | Release archives: install, `bsx update` self-upgrade, cargo-binstall, first server run |
| [docker.md](docker.md) | Compose deployment: volumes, users, secrets, HTTPS, capability switches |
| [turso.md](turso.md) | Native Turso FTS and optional Tantivy: build switches, query syntax, index rebuild |

## Documentation standards

All documents in this directory follow the
[Google developer documentation style guide](https://developers.google.com/style)
highlights below. Keep them applying when you edit:

- **Write for the reader**: second person ("run the server"), present tense,
  active voice, no marketing adjectives.
- **Sentence case** for headings and UI labels; code identifiers stay in
  their exact case (`brains-server`, `BRAINS_TOKEN`).
- **One topic per page**; cross-link instead of repeating. Procedures are
  numbered steps; reference material uses tables and definition lists.
- **Every code block declares its language** and is copy-pasteable with the
  stated prerequisites.
- **Markdown**: GitHub-flavored. ATX headings (`#`), fenced code blocks,
  reference tables for enumerable facts. No emoji in documentation; no HTML.
- **Terminology is fixed**: HEAD (the per-path pointer), revision (immutable
  record), tombstone (soft delete), vault (the store), `bsx` (the CLI).
- **State limits honestly**: if something is not verified or not supported,
  say so on the page.

Regenerating generated artifacts referenced here (OpenAPI, SDKs) is
documented in [../scripts/gen-api.sh](../scripts/gen-api.sh).
