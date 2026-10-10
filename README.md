# Brains

<p align="left"><img src="apps/web/public/favicon.svg" width="40" alt="Brains logo" /></p>

**A human-readable, migration-safe knowledge store — plain files in, plain files out, every revision kept.**

[中文文档](README.zh.md) · [Architecture](docs/architecture.md) · [Runbook](docs/runbook.md) · [Import, sync & migration](docs/portable-workflow.md) · [Verification log](docs/verification.md)

[![CI](https://github.com/unic/brains/actions/workflows/ci.yml/badge.svg)](https://github.com/unic/brains/actions/workflows/ci.yml)

Brains is *not* an AI memory framework. There is no recall engine, no vector database, no context assembly. It is a versioned store for the notes you already write, built on one idea: **the database may disappear — your files and their history must not.**

| Login | Workbench |
|---|---|
| ![login](docs/screenshots/01-login.png) | ![workspace](docs/screenshots/02-workspace.png) |
| *Full-text search* | *Three-way conflict view — split/inline diff, nothing is ever auto-overwritten* |
| ![search](docs/screenshots/03-search.png) | ![conflict](docs/screenshots/04-conflict.png) |
| *Dark theme* | |
| ![dark](docs/screenshots/05-dark.png) | |

The web UI ships with **dark / light / system themes** and **i18n (zh-CN / English, namespace-based via i18next)** — switchable from the header; the choice persists per browser.

## Core properties

- **Ships with content, not a blank screen.** First boot on an empty vault seeds five template notes — a welcome page, the usage guide, an **AI collaboration guide**, a **self-contained memory-writing guide** and an inbox — so humans and agents start from working conventions (cloud instances: `bsx seed`).
- **AI-friendly by construction.** [docs/ai-integration.md](docs/ai-integration.md) is a machine-first API reference (endpoints, CAS etiquette, error model), `llms.txt` indexes the project for crawlers/agents, and every vault carries the condensed etiquette inside itself (`docs/ai-guide.md`).

- **Immutable revisions + one atomic commit point.** Every save writes a content-addressed blob and a revision record, then swaps the per-path HEAD with a compare-and-swap. Two writers racing on the same base: exactly one commits, the loser gets `412` with the current revision — never a silent overwrite.
- **Conflicts are a first-class UI.** The conflict view offers VSCode-style **split** (remote vs local side-by-side with synced chunk highlights) and **inline** (per-chunk accept/reject against the remote original) diff modes, both editable; keep-local or take-remote is an explicit human choice, and it re-conflicts if the remote moves again mid-decision. While typing, a one-click **inline diff against the saved base** shows exactly what is unsaved, with per-chunk revert.
- **A real editor, not a textarea.** CodeMirror with find & replace (match case / regex / whole word, replace all), selection-match highlights, and a folder tree sidebar with **multi-select batch actions** (batch trash / batch restore).
- **Delete is a tombstone; restore is a new revision.** Generations only move forward. Nothing is ever destroyed by accident, and the history panel can bring any contentful revision back.
- **Local and cloud profiles sync in both directions.** `bsx sync` uses per-file revision bases and CAS, merges non-overlapping text edits, and leaves divergent files untouched for an explicit resolution.
- **Portable current files and complete reachable history.** `bsx import/export` moves readable working trees; `--format archive` preserves reachable revisions and blobs across FS/Turso and R2/D1. `profile use --verify-with` validates content and the rebuilt index before selecting the daily profile. See the [portable workflow](docs/portable-workflow.md) for the different guarantees, resumable failures, shell completion and Git-aware build versions.
- **The on-disk layout is the backup.** `vaults/{vault}/heads|revisions|blobs` — copy the directory and you have a complete raw backup; the index (Turso/D1) is a rebuildable projection you can delete at any time.
- **One HTTP contract, three clients.** The Web app, remote CLI and `curl`/agents speak the same versioned REST API; standalone CLI calls the same application layer directly, backed by the same axum layer on both runtimes.
- **Bounded everything.** Literal-scan windows never fake a complete search; a lost index degrades loudly (`503` / `building`), and oversized text is stored but never indexed.

[Markdown metadata and preview guide (中文)](docs/markdown-preview.md)

## Quickstart: standalone bsx

```bash
mbx install --path apps/cli --locked --force
bsx init "$HOME/.brains/my-library" --profile laptop
bsx import ./notes --dry-run
bsx import ./notes
bsx ls
bsx search "hello"
```

This is `local` mode: bsx opens the native stores directly, without a server,
listener or token. `init` creates an empty library and selects its profile;
run `bsx seed` if you want the template notes.

Add a `remote` profile when you want a Web UI or shared server. Loopback and
public servers use the same commands:

```bash
bsx profile add cloud --url "$CLOUD_URL"
printf '%s' "$CLOUD_TOKEN" | bsx --profile cloud login --stdin-token
bsx --profile laptop sync --remote-profile cloud
```

Sync exchanges current state; use `export/import --format archive` for complete
reachable history. See [local and remote workflows](docs/local-remote.md) for
standalone use, server handoff and multiple servers. One process owns a data
directory at a time: while a server owns it, access it through a remote profile.
See the [runbook](docs/runbook.md) for server startup, auth, backups and recovery.

## Cloudflare local simulation (no cloud account needed)

The same axum layer runs as a Worker over R2 (content) + D1 (rebuildable index). `wrangler dev` executes it in a local workerd with simulated R2/D1 — the protocol behavior is the production behavior:

```bash
rustup target add wasm32-unknown-unknown
cargo install worker-build --locked
pnpm --dir apps/web build
cd apps/worker
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release
pnpm exec wrangler d1 migrations apply BRAINS_INDEX --local
pnpm exec wrangler dev --port 8799          # → http://localhost:8799
bash tests/worker-smoke.sh                  # CAS race, search, trash/restore acceptance
```

Validated acceptance (local workerd): the **full 24-check Web suite passes against the Worker unchanged**, alongside concurrent same-base writes → exactly one `200`, one `412`; trash→410→restore yielding a new generation; deleting the D1 database and re-running `reindex` restoring identical search results; CLI + export working unchanged against the Worker. Real-account deployment is the remaining milestone and is deliberately not claimed as done ([verification log](docs/verification.md)).

## Architecture

```
Browser ──/ (static)──▶ Workers Static Assets │ native Axum + web dist
remote CLI / curl ──/api/v1/*──┐
                        ▼
        shared axum layer (crates/api-http): routes · error mapping · auth
                        │
      ┌─────────────────┴──────────────────┐
      ▼ local-rust profile                 ▼ cloudflare-rust profile
  filesystem + locks (HEAD CAS)        R2 binding (HEAD CAS via storage ETag)
  Turso + optional Tantivy             D1 binding (SQLite FTS5)
      apps/server                          apps/worker (workers-rs)
```

| Crate | Role |
|---|---|
| `crates/domain` | paths, HEAD/Revision, error taxonomy, capability report |
| `crates/format` | bounded frontmatter/Markdown parsing, link extraction |
| `crates/ports` | store traits — CAS heads, create-only blobs/revisions, index (deliberately `Send`-free; the Worker bridges its `!Send` futures) |
| `crates/application` | the whole business orchestration: commit pipeline, search, restore, patch, copy/move, reindex |
| `crates/api-http` | shared HTTP layer (routing, error mapping, auth) for both runtimes |
| `crates/adapter-fs` / `adapter-turso` | local profile adapters |
| `crates/adapter-r2` / `adapter-d1` | Worker profile adapters (same key layout, same SQL) |
| `crates/local` | native store assembly shared by CLI and server, without HTTP or auth |
| `crates/client` | Library interface with local/HTTP adapters, shared archive and sync |
| `apps/web` | React + CodeMirror editor (explicit load/clean/dirty/saving/conflict state machine) |
| `apps/server` · `apps/worker` | thin runtime wiring per profile |

## Testing

- **91 Rust tests** — domain, format, adapters, 31 application business tests (real fs+Turso, injected CAS/index failures), HTTP + CLI contract tests.
- **24 Web checks (22 browser E2E + 2 parsing checks)** — SerenityJS screenplay pattern against the real server: login, authoring, CAS conflict + re-conflict, literal/FTS search, trash/restore, history restore.
- **Standalone CLI business scenarios and snapshot** — offline edits/history, pagination, directory ownership, and multi-server collaboration (`tests/local-workflows.py`); both local/remote sources exercise archive and sync against Worker.
- **Worker business smoke** — the same acceptance list executed against workerd (`tests/worker-smoke.sh`).

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
pnpm --dir apps/web build && pnpm --dir tests/e2e exec playwright test   # needs BRAINS_SERVER_BIN/BRAINS_WEB_DIST
bash tests/worker-smoke.sh                                               # against a running `wrangler dev`
```

## CI · Release · Deploy

| Workflow | Trigger | What it does |
|---|---|---|
| [ci.yml](.github/workflows/ci.yml) | push / PR | fmt · clippy `-D warnings` · Rust tests · SDK codegen drift check · wasm check · web build · screenplay E2E (native **and** Worker) · workerd business smoke |
| [release.yml](.github/workflows/release.yml) | tag `v*` **or** manual dispatch | builds CLI+server (macOS arm64/x64, Linux musl), web dist, Worker bundle → publishes a GitHub release with SHA-256s. One click: *Actions → Release → Run workflow → version* |
| [deploy.yml](.github/workflows/deploy.yml) | manual dispatch | builds the Worker and deploys to Cloudflare (`wrangler deploy` + remote D1 migrations). Requires `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` secrets and the one-time provisioning documented in the file header |
| dependabot | weekly | cargo · npm · actions updates |

## Status & honest limits

- local-rust profile: **complete and verified** (P0 + patch/copy/move/diff/export; the four unsupported capabilities answer `501` and report `false`).
- cloudflare-rust profile: **implemented and verified under local workerd**; real-account deployment, real R2/D1 behavior and the free-tier CPU budget remain to be validated with credentials.
- Windows and non-Chromium browsers are untested; no long-running concurrency soak yet.

## License

MIT

Native search now uses Turso 0.8.2 with optional Tantivy. See [build features and search semantics](docs/turso.md).
