# Architecture

This document describes Brains as implemented in this repository: the crate
graph, the storage format, the commit protocol, the auth model, standalone access, and the two server
runtimes. For operational procedures (run, back up, rebuild the
index), see [runbook.md](runbook.md). For a machine-readable API contract,
see [../api/brains.idl](../api/brains.idl) and
[../sdk/openapi/brains-openapi.json](../sdk/openapi/brains-openapi.json).

## Design invariants

1. **Immutable history.** Blobs and revisions are content-addressed and never
   modified after write. "Undo" means restoring an old revision as a new one;
   generations only move forward.
2. **One atomic commit point per path.** The per-path HEAD record is swapped
   with a compare-and-swap (CAS). Two writers on the same base: exactly one
   `200`, the loser gets `412` with the current revision.
3. **Delete is a tombstone.** Trashing writes a new revision that marks the
   path deleted; nothing is destroyed. Restore reads back through history.
4. **The directory layout is the backup.** Copying the vault directory is a
   complete raw backup. Indexes (Turso/D1) are rebuildable projections.
5. **Honest degradation.** A missing or stale index answers `building`/`503`
   instead of faking complete results; listings report truncation.

## Crate graph

Dependencies point toward the domain core; nothing in `domain` knows about
HTTP, filesystems, SQLite, or Cloudflare.

```
domain ← format ← ports ← application ← api-http ← apps/server (native)
                              ↑              ↖ apps/worker (cloudflare)
            adapter-fs ───────┤
            adapter-turso ───┤        client ← apps/cli (bsx)
            adapter-r2 ───────┤
            adapter-d1 ───────┘        sdk/rust (generated, external consumers)

api-types: HTTP DTOs shared by api-http and client
```

| Crate | Responsibility |
|---|---|
| `brains-domain` | Vault paths, HEAD/revision records, scopes and principals, limits, signed cursors, error taxonomy |
| `brains-format` | Frontmatter + markdown parsing, title extraction, `[[link]]` extraction |
| `brains-ports` | Storage traits (`HeadStore`, `RevisionStore`, `BlobStore`, `IndexStore`) declared as `-> impl Future + Send` (RPITIT) |
| `brains-application` | The single commit protocol, queries, search, links, reindex, patch, copy/move, first-boot template seeding |
| `brains-adapter-fs` | Native storage: one JSON file per HEAD/revision, content-addressed blobs with hardlink dedup, path locks |
| `brains-adapter-turso` | Native Turso index (optional Tantivy FTS + literal windows) |
| `brains-adapter-r2` | Cloudflare R2 storage: ETag as CAS token, create-only via `If-None-Match: *` |
| `brains-adapter-d1` | Cloudflare D1 index; shared projection and literal query, SQLite FTS5 schema |
| `brains-api-types` | Transport DTOs (snake_case JSON), the HTTP surface shapes |
| `brains-api-http` | The shared axum layer: routing, auth middleware, preconditions, ETag helpers, error mapping. Both runtimes serve this one router |
| `brains-local` | Native FS/Turso assembly for server and standalone CLI; no HTTP, auth or background runtime |
| `brains-client` | Library interface; HTTP and direct local adapters; shared archive, sync and export |
| `brains-server` | Native binary: config, listener, SPA hosting, first-boot seeding |
| `brains-worker` | Cloudflare Worker binary: per-request bindings (R2/D1/env secrets), Static Assets for the SPA |
| `brains-cli` | The `bsx` command line client |
| `brains-sdk` | Generated Rust client SDK (from the IDL, for external consumers) |

## Standalone and remote access

CLI commands depend on `Library`, implemented by `LocalClient` and `BrainClient`.
LocalClient calls application services through `brains-local` native assembly;
BrainClient uses HTTP. FileEndpoint and ArchiveEndpoint keep one implementation
of import/export/sync for both modes. Application owns query normalization and
write rules; HTTP owns authentication and wire conversion.

A native data directory has one process owner. FsStore clones retain the OS lock
for the lifetime of all adapters; Vault closes its index before releasing these
handles. A server holds ownership while running; use its remote profile during
that time. Different directories can collaborate through existing sync/archive.
Each vault owns its Turso index. A missing index is built from all authoritative
HEADs into a temporary DB, checkpointed and closed with error checks, then renamed.
No legacy root index is read. This does not provide a transaction across HEAD and
index or serialize concurrent requests inside one server.

## Storage layout

The on-disk layout is identical across runtimes and is the raw backup format:

```
<data-dir>/
  vaults/<vault>/
    format.json            # {"format": "brains", ...}
    heads/<path>.json      # one HEAD per path — the only mutable record (CAS)
    revisions/<hex>.json   # immutable revision metadata (parent chain)
    blobs/<hex>/<name>     # immutable content, content-addressed
    locks/                 # path-level exclusive locks (native profile)
    index.turso-fts.db*              # per-vault rebuildable Turso index
  .brains.lock             # directory ownership for CLI/server
  auth.json  secret.key    # token registry / session signing key
```

On Cloudflare the same keys live in one R2 bucket, and the index lives in D1.
The HEAD record carries `revision`, `generation`, and `state`
(`live`/`trashed`); it is the single serialization point for writers.

## Commit protocol

Every mutation flows through one protocol in `brains-application`:

1. **Validate**: scope check (`Principal` scopes vs path), text decode check,
   size limits.
2. **Write blob** (content-addressed, create-only).
3. **Write revision record** (parent = previous HEAD revision, action =
   `put`/`trash`/`restore`/`copy`/`patch`).
4. **Swap HEAD with CAS**: native uses a file lock + compare; R2 uses ETag
   conditional puts. Losers surface as `412 REVISION_CONFLICT` with the
   current revision.
5. **Project to index** (best effort): upsert searchable rows or delete them
   for tombstones. Index state (`ready`/`building`) is reported with every
   mutation, and `X-Operation-ID` makes retries idempotent.

Reads never touch locks: `GET` returns HEAD + revision content with a strong
ETag (`"revision-hex"`); conditional `GET` honors `If-None-Match` with a weak
comparison and answers `304`.

## Index as a projection

The index (Turso locally, D1 on Cloudflare) holds searchable projections:
entry metadata, FTS tokens, tag/title frontmatter, and wiki-links. It can be
deleted and rebuilt at any time (`POST /maintenance/reindex` or
`bsx reindex --all-pages`) from authoritative storage. Literal search uses
bounded scan windows and never reports `complete` falsely.

## Auth model

- **Tokens**: the root token registers into `auth.json` (native) or
  `TOKEN_REGISTRY_JSON` (cloud, sha256-hashed). Scopes are path prefixes:
  `read:/`, `write:/`, `admin:/`.
- **Sessions**: the web UI exchanges a token for an HMAC-signed cookie
  (`brains_session`, HttpOnly, SameSite=Lax) plus a CSRF token; cookie-driven
  mutations require the CSRF header and an allowed Origin.
- **API clients**: `Authorization: Bearer <token>` on every call.

## API contract

The wire contract is defined once in [`api/brains.idl`](../api/brains.idl)
(xidl syntax) and committed artifacts are generated from it
(`scripts/gen-api.sh`, requires `xidlc`):

- `sdk/openapi/brains-openapi.json` — OpenAPI 3.1 document.
- `sdk/rust` — bearer-token Rust client crate (`brains-sdk`).
- `sdk/typescript` — TypeScript client (runtime packages are published to the
  xidl project's GitHub Packages registry).

The server implementation is handwritten axum shared by both runtimes. Three
wire semantics are beyond the current IDL and are documented per method in
the `.idl` file: conditional `GET` (304 + ETag), response headers on error
responses, and per-response dynamic Content-Type on `GET /file`.

## Runtime profiles

| | native (`brains-server`) | cloudflare (`brains-worker`) |
|---|---|---|
| Storage | local FS | R2 (same key layout) |
| Index | Turso (optional Tantivy) | D1 (SQLite FTS5) |
| Auth store | `auth.json` + `secret.key` | `TOKEN_REGISTRY_JSON` + signing key secrets |
| Static assets | `--web-dist` served by axum | Workers Static Assets |
| HTTP layer | `brains-api-http::build_router` | the same `build_router` |

Because both runtimes serve one router, the HTTP contract, error mapping, and
auth semantics cannot drift between them. The full browser e2e
suite runs against both.
