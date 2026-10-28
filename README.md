<h1 align="left">
  <img src="apps/web/public/favicon.svg" width="64" height="64" align="middle" alt="" />
  <img src="docs/assets/brains-signature.svg" width="240" height="117" align="middle" alt="Brains" />
</h1>

**A human-readable, migration-safe knowledge store — plain files in, plain files out, every revision kept.**

[中文文档](README.zh.md) · [Architecture](docs/architecture.md) · [Runbook](docs/runbook.md) · [Import, sync & migration](docs/portable-workflow.md)

[![CI](https://github.com/YuniqueUnic/brains/actions/workflows/ci.yml/badge.svg)](https://github.com/YuniqueUnic/brains/actions/workflows/ci.yml)

Brains is *not* an AI memory framework. There is no recall engine, no vector database, no context assembly. It is a versioned store for the notes you already write, built on one idea: **the database may disappear — your files and their history must not.**

| Login | Workbench |
|---|---|
| ![login](docs/screenshots/01-login.png) | ![workspace](docs/screenshots/02-workspace.png) |
| *Full-text search* | *Three-way conflict view — split/inline diff, nothing is ever auto-overwritten* |
| ![search](docs/screenshots/03-search.png) | ![conflict](docs/screenshots/04-conflict.png) |
| *Dark theme* | |
| ![dark](docs/screenshots/05-dark.png) | |

The web UI ships with **dark / light / system themes** and **i18n (zh-CN / English, namespace-based via i18next)** — switchable from the header; the choice persists per browser.

## Live demo

Try the [browser demo](https://YuniqueUnic.github.io/brains/). No account or server is needed. Demo files exist only in the current page and reset on refresh; local editor drafts may remain in your browser. Search uses a simplified in-memory implementation.

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

Install without compiling (prebuilt release binaries, kept current with
`bsx update --check`), or build from source with cargo:

```bash
cargo binstall --git https://github.com/YuniqueUnic/brains brains-cli
bsx update            # later: self-upgrade from GitHub releases (SHA-256 verified)
# or: cargo install --locked --git https://github.com/YuniqueUnic/brains brains-cli
```

```bash
bsx init "$HOME/.brains/my-library" --profile laptop
bsx import ./notes --dry-run
bsx import ./notes
bsx ls
bsx search "hello"
```

This is `local` mode: bsx opens the native stores directly, without a server,
listener or token. `init` creates an empty library and selects its profile;
run `bsx seed` if you want the template notes.

What a shipped `bsx` can do (full matrix in [docs/capabilities.md](docs/capabilities.md)):

| Command | What it does |
|---|---|
| `bsx search --mode hybrid` | natural-language rank fusion over literal + FTS and a configured Worker semantic provider; source revisions, provenance and incomplete coverage are explicit |
| `bsx ls --status draft --type Decision --stale` | lifecycle/trust filters over the indexed projection (`status`, `type`, `trust`, `stale_after`) |
| `bsx check` | quality report: stale notes, unconfirmed agent claims, broken links, footnote gaps (`--okf` for OKF v0.2 conformance) |
| `bsx checkpoint show/update <scope>` | per-scope continuity note with CAS rules |
| `bsx convert <attachment>` | PDF/DOCX/XLSX/PPTX/HTML/CSV/TXT → Markdown companion note (offline, embedded converter) |
| `bsx import-url <url> <path>` | web page → Markdown through a strict fetch ladder (SSRF-guarded, robots-aware) |
| `bsx checkout/status/pull/push` | tracked working copy with three-way merges and conflict reporting |
| `bsx seed --refresh` | re-seed template notes into older vaults (content-diffed, CAS-guarded) |
| `bsx update` | self-upgrade from GitHub releases (SHA-256 verified, atomic replace) |

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

For a real Cloudflare deployment, use the [dotenv and GitHub Actions deployment guide](docs/cloudflare.md). Local and Actions deployments share one entry point. Optional cloud conversion, URL import, quota admission and managed semantic retrieval use the same protected API; see [cloud architecture](docs/cloudflare-ai-plan.md).

Review [cost amplification and controls](docs/cloudflare-cost-safety.md) before enabling managed services or increasing limits. It covers reported billing incidents, authentication before metered work, bounded scans, failed-job recovery and the limits of application accounting.

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

Rust build profiles are shared at the workspace root: `dev` keeps source locations and incremental compilation; `release` optimizes size with Thin LTO and symbol stripping. The native E2E job runs its feature combinations, CLI workflows and browser scenarios against release binaries. See [build commands and debugging trade-offs](docs/releasing.md#开发与发布构建). Native search runs on Turso with optional Tantivy — see [build features and search semantics](docs/turso.md).

- **Rust tests** — domain, format, adapters, application business tests (real fs+Turso, injected CAS/index failures), HTTP + CLI contract tests.
- **Browser E2E (SerenityJS screenplay)** — against a real server and against workerd: login, authoring, CAS conflict + re-conflict, literal/FTS/hybrid search, trash/restore, history restore.
- **Standalone CLI business scenarios and snapshot** — offline edits/history, pagination, directory ownership, and multi-server collaboration (`tests/local-workflows.py`); both local/remote sources exercise archive and sync against Worker.
- **Worker business smoke** — the same acceptance list executed against workerd (`tests/worker-smoke.sh`).

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
pnpm lint
pnpm --dir apps/web build && pnpm --dir tests/e2e exec playwright test   # needs BRAINS_SERVER_BIN/BRAINS_WEB_DIST
bash tests/worker-smoke.sh                                               # against a running `wrangler dev`
```

## CI · Release · Deploy

| Workflow | Trigger | What it does |
|---|---|---|
| [ci.yml](.github/workflows/ci.yml) | push / PR | fmt · check · clippy `-D warnings` · Rust tests · SDK codegen drift check · wasm check · JS/TS lint · web build · screenplay E2E (native **and** Worker) · workerd business smoke |
| [pages.yml](.github/workflows/pages.yml) | push / PR / manual | builds the demo and runs 6 browser scenarios; only `main` deploys to Pages. |
| [release-please.yml](.github/workflows/release-please.yml) | `main` push / manual | one release PR updates all product versions; merging creates the tag and release notes, then dispatches the asset build. See [releasing](docs/releasing.md). |
| [release.yml](.github/workflows/release.yml) | dispatch from `main`, with an existing release tag as input | builds CLI+server (macOS arm64/x64, Linux musl, Windows x64), web dist and Worker bundle, verifies the tag and uploads assets with SHA-256s. |
| [deploy.yml](.github/workflows/deploy.yml) | manual dispatch | builds the Worker and deploys to Cloudflare (`wrangler deploy` + remote D1 migrations). Requires `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` secrets and the one-time provisioning documented in the file header |
| dependabot | weekly | cargo · npm · actions updates |

`main` accepts changes through pull requests only. `Development gates` and `Build web demo` must pass against the latest `main`; force pushes and branch deletion are blocked, including for administrators. Prefer rebase merging; resolve review threads before merging. A second reviewer is not required for this personal repository.

## Status & honest limits

- local-rust profile: **complete and verified** (P0 + patch/copy/move/diff/export, lifecycle metadata, hybrid search, document conversion, URL import, quality checks, working copy).
- cloudflare-rust profile: **implemented and verified under local workerd and on isolated Cloudflare resources**, including real R2/D1 operations, conversion, URL import, semantic retrieval and read-only MCP. Production rollout, Access SSO and sustained Free-tier CPU performance remain separate checks; see the [verification record](docs/cloudflare-verification.md).
- Windows x64 has native CI for CLI/server workflows and ZIP packaging; see [native releases](docs/native-release.md) for NTFS, update and durability requirements. Windows ARM64, Windows service installation and non-Chromium browsers are not covered; no long-running concurrency soak yet.

## License

[MIT](LICENSE)

## Docker

Run the server and bundled web app with persistent storage using `docker compose up -d`. Linux AMD64 and ARM64 images are published with releases; the image ships the full capability build, and `BRAINS_DISABLE=document_conversion,semantic` stops advertising selected capabilities. See [Docker deployment](docs/docker.md) for volumes, bind mounts, non-root permissions, secrets, HTTPS and source builds, and [capabilities](docs/capabilities.md) for the control model.
