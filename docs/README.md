# Brains documentation

Documentation for the Brains repository, written against the code as it is
(not as it was drafted). Start here:

| Document | Contents |
|---|---|
| [architecture.md](architecture.md) | How the system works: crates, storage layout, commit protocol, auth, API contract, runtimes |
| [runbook.md](runbook.md) | How to run, configure, back up, deploy, and troubleshoot |
| [ai-integration.md](ai-integration.md) | Agent-facing API reference and write etiquette |
| [verification.md](verification.md) | What was verified, when, and with what limits |

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

- [原生 Turso 与可选全文检索](turso.md)：构建开关、搜索语法、索引重建及上游能力边界。
