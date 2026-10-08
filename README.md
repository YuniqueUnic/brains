# BrainStore

**A human-readable, migration-safe knowledge store — plain files in, plain files out, every revision kept.**

[中文文档](README.zh.md) · [Design spec (CN)](brainstore-design/BrainStore-Architecture-v1.md) · [Runbook](docs/runbook.md) · [Verification log](docs/verification.md)

[![CI](https://github.com/unic/brains/actions/workflows/ci.yml/badge.svg)](https://github.com/unic/brains/actions/workflows/ci.yml)

BrainStore is *not* an AI memory framework. There is no recall engine, no vector database, no context assembly. It is a versioned store for the notes you already write, built on one idea: **the database may disappear — your files and their history must not.**

| Login | Workbench |
|---|---|
| ![login](docs/screenshots/01-login.png) | ![workspace](docs/screenshots/02-workspace.png) |
| *Full-text search* | *Three-way conflict view — nothing is ever auto-overwritten* |
| ![search](docs/screenshots/03-search.png) | ![conflict](docs/screenshots/04-conflict.png) |

## Core properties

- **Immutable revisions + one atomic commit point.** Every save writes a content-addressed blob and a revision record, then swaps the per-path HEAD with a compare-and-swap. Two writers racing on the same base: exactly one commits, the loser gets `412` with the current revision — never a silent overwrite.
- **Conflicts are a first-class UI.** The editor shows base / local / remote side by side; keep-local or take-remote is an explicit human choice, and re-conflicts if the remote moves again mid-decision.
- **Delete is a tombstone; restore is a new revision.** Generations only move forward. Nothing is ever destroyed by accident, and the history panel can bring any contentful revision back.
- **The on-disk layout is the backup.** `vaults/{vault}/heads|revisions|blobs` — copy the directory and you have a complete raw backup; the index (SQLite/D1) is a rebuildable projection you can delete at any time.
- **One HTTP contract, three clients.** The Web app, the Rust CLI and `curl`/agents all speak the same versioned REST API, backed by the same axum layer on both runtimes.
- **Bounded everything.** Literal-scan windows never fake a complete search; a lost index degrades loudly (`503` / `building`), and oversized text is stored but never indexed.

## Quickstart (local profile)

```bash
cargo build -p brainstore-server -p brainstore-cli
pnpm --dir apps/web build

# boot the whole stack (API + web UI, same origin)
./target/debug/brainstore-server \
  --data-dir ./brain-data \
  --token "$MY_TOKEN" \
  --web-dist apps/web/dist
# → http://127.0.0.1:8787  (paste your token into the login screen)
```

CLI against the same server:

```bash
./target/debug/brainstore-cli profile add local --url http://127.0.0.1:8787 --token "$MY_TOKEN"
./target/debug/brainstore-cli ls ""
echo "# hello" > /tmp/hello.md
./target/debug/brainstore-cli put notes/hello.md --from /tmp/hello.md --create
./target/debug/brainstore-cli search "hello"
```

See the [runbook](docs/runbook.md) for auth model, backup layout, reindex and troubleshooting.

## Cloudflare local simulation (no cloud account needed)

The same axum layer runs as a Worker over R2 (content) + D1 (rebuildable index). `wrangler dev` executes it in a local workerd with simulated R2/D1 — the protocol behavior is the production behavior:

```bash
rustup target add wasm32-unknown-unknown
cargo install worker-build --locked
pnpm --dir apps/web build
cd apps/worker
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release
pnpm exec wrangler d1 migrations apply BRAIN_INDEX --local
pnpm exec wrangler dev --port 8799          # → http://localhost:8799
bash tests/worker-smoke.sh                  # CAS race, search, trash/restore acceptance
```

Validated acceptance (local workerd): the **full 12-scenario browser E2E suite passes against the Worker unchanged**, alongside concurrent same-base writes → exactly one `200`, one `412`; trash→410→restore yielding a new generation; deleting the D1 database and re-running `reindex` restoring identical search results; CLI + export working unchanged against the Worker. Real-account deployment is the remaining milestone and is deliberately not claimed as done ([verification log](docs/verification.md)).

## Architecture

```
Browser ──/ (static)──▶ Workers Static Assets │ native Axum + web dist
CLI / curl ──/api/v1/*──┐
                        ▼
        shared axum layer (crates/api-http): routes · §11.3 errors · §15 auth
                        │
      ┌─────────────────┴──────────────────┐
      ▼ local-rust profile                 ▼ cloudflare-rust profile
  filesystem + locks (HEAD CAS)        R2 binding (HEAD CAS via storage ETag)
  SQLite (rebuildable index)           D1 binding (same SQL, rebuildable)
      apps/server                          apps/worker (workers-rs)
```

| Crate | Role |
|---|---|
| `crates/domain` | paths, HEAD/Revision, error taxonomy, capability report |
| `crates/format` | bounded frontmatter/Markdown parsing, link extraction |
| `crates/ports` | store traits — CAS heads, create-only blobs/revisions, index (Send-free by design §9.4) |
| `crates/application` | the whole business orchestration: commit pipeline, search, restore, patch, copy/move, reindex |
| `crates/api-http` | shared HTTP layer (routing, error mapping, auth) for both runtimes |
| `crates/adapter-fs` / `adapter-sqlite` | local profile adapters |
| `crates/adapter-r2` / `adapter-d1` | Worker profile adapters (same key layout, same SQL) |
| `crates/client` | typed Rust HTTP client (CLI): conditional writes, two-phase export |
| `apps/web` | React + CodeMirror editor state machine (design §13.2) |
| `apps/server` · `apps/worker` | thin runtime wiring per profile |

## Testing

- **76 Rust tests** — domain, format, adapters, 24 application business tests (real fs+SQLite, injected CAS/index failures), HTTP + CLI contract tests.
- **12 browser E2E** — SerenityJS screenplay pattern against the real server: login, authoring, CAS conflict + re-conflict, literal/FTS search, trash/restore, history restore.
- **Worker business smoke** — the same acceptance list executed against workerd (`tests/worker-smoke.sh`).

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
pnpm --dir apps/web build && pnpm --dir tests/e2e exec playwright test   # needs BRAIN_SERVER_BIN/BRAIN_WEB_DIST
bash tests/worker-smoke.sh                                               # against a running `wrangler dev`
```

## CI · Release · Deploy

| Workflow | Trigger | What it does |
|---|---|---|
| [ci.yml](.github/workflows/ci.yml) | push / PR | fmt · clippy `-D warnings` · 76 tests · wasm check · web build · screenplay E2E (native **and** Worker) · workerd business smoke |
| [release.yml](.github/workflows/release.yml) | tag `v*` **or** manual dispatch | builds CLI+server (macOS arm64/x64, Linux musl), web dist, Worker bundle → publishes a GitHub release with SHA-256s. One click: *Actions → Release → Run workflow → version* |
| [deploy.yml](.github/workflows/deploy.yml) | manual dispatch | builds the Worker and deploys to Cloudflare (`wrangler deploy` + remote D1 migrations). Requires `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` secrets and the one-time provisioning documented in the file header |
| dependabot | weekly | cargo · npm · actions updates |

## Status & honest limits

- local-rust profile: **complete and verified** (P0 + patch/copy/move/diff/export; the four unsupported capabilities answer `501` and report `false`).
- cloudflare-rust profile: **implemented and verified under local workerd**; real-account deployment, real R2/D1 behavior and the free-tier CPU budget (§16) remain to be validated with credentials.
- Windows and non-Chromium browsers are untested; no long-running concurrency soak yet.

## License

MIT
