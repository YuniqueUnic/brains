# BrainStore — AI Integration Guide

> Machine-first reference: everything an AI agent needs to read, write and
> deploy against a BrainStore instance. Human-oriented docs live in
> [README.md](../README.md) and [docs/runbook.md](runbook.md).

## What BrainStore is (one paragraph)

BrainStore is a versioned document store with plain-file semantics: every save
writes an immutable blob + revision, then atomically moves a per-path HEAD
pointer with a compare-and-swap. Two writers on the same base → exactly one
`200`, the loser gets `412` with the current revision. Deletes are tombstones;
restores create new revisions. The on-disk layout (`vaults/{vault}/heads |
revisions | blobs`) *is* the backup; search indexes (SQLite/D1) are rebuildable
projections. It is **not** an AI memory framework — no recall engine, no
embeddings, no context assembly.

## Connection

```bash
BASE="http://127.0.0.1:8787"          # local server, or your deployed Worker URL
TOKEN="..."                            # root token (server prints it on first boot)
AUTH="Authorization: Bearer $TOKEN"
```

Login exchanges a token for a short-lived session cookie + CSRF token (for
browser flows). Agents can simply use the bearer token on every call.

## Endpoints (v1, all under `/api/v1`)

| Method + path | Purpose | Preconditions / notes |
|---|---|---|
| `GET /capabilities` | feature flags, limits, vault name | no auth needed? — auth required |
| `GET /files?source=store\|index&prefix=&limit=&cursor=` | list; `store` = authoritative, `index` = tag/kind/sort filters | cursor is **opaque and signed**; echo it back unchanged, changing filters invalidates it (400) |
| `GET /file?path=P` | raw bytes; `ETag` header = current revision | `If-None-Match` supported (weak compare, 304) |
| `GET /file?path=P&format=meta` | JSON metadata (revision, generation, state, sha256, media_type) | 410 Gone once trashed |
| `PUT /file?path=P` | create/update bytes | create: `If-None-Match: *`; update: `If-Match: "<revision>"`. `X-Content-SHA256` (hex) **required** for non-text, verified server-side |
| `DELETE /file?path=P` | soft delete (tombstone) | `If-Match: "<revision>"` |
| `POST /restore` | restore a prior revision as a NEW revision | body `{path, revision}`, `If-Match` on the current head |
| `GET /revisions?path=P` | reachable history, newest first | signed cursor pagination |
| `GET /search?q=&mode=literal\|fts&prefix=` | literal = case-sensitive UTF-8 substring (complete or honestly incomplete); FTS = tokenized, needs index | `423`/`building` while index is behind — retry or use literal |
| `GET /links?path=P&direction=outbound\|backlinks` | wiki-link graph | |
| `POST /maintenance/reindex` | rebuild index projection | safe anytime |
| `POST /copy`, `POST /move`, `GET /diff` | transfer between paths | move is CAS on the source |
| `POST /session`, `DELETE /session` | login/logout (browser) | |

## Write etiquette for agents

1. **Search before you write** — avoid duplicate topics:
   `GET /search?q=<keyword>&mode=fts`.
2. **One topic per file**; the filename is the address of the idea. Split long
   notes and cross-link with `[[wiki-link]]` (backlinks are served).
3. **Conditional writes only.** Create with `If-None-Match: *`; update with
   `If-Match: "<revision>"`. On `412` (`REVISION_CONFLICT`, body carries
   `current_revision`): re-read, merge, retry. Never blind-overwrite.
4. **Idempotent retries**: send `X-Operation-ID: <32 hex>` on mutations;
   repeating the same operation id is a no-op, not a duplicate.
5. **History is immutable.** "Undo" = restore an old revision (creates a new
   revision). There is no rewrite, ever.
6. **Frontmatter**: start notes with `title` and `tags` — indexing and the
   tag filter rely on them.
7. **Text limits**: text bodies above `max_text_bytes` (see capabilities) are
   stored but not indexed; attachments above `max_attachment_bytes` are
   rejected.

## Minimal end-to-end example

```bash
# create
REV=$(curl -s -X PUT -H "$AUTH" -H "If-None-Match: *" \
  -H "Content-Type: text/markdown" --data-binary "# topic" \
  "$BASE/api/v1/file?path=notes/topic.md" | jq -r .revision)

# update (CAS)
curl -s -X PUT -H "$AUTH" -H "If-Match: \"$REV\"" \
  -H "Content-Type: text/markdown" --data-binary "# topic v2" \
  "$BASE/api/v1/file?path=notes/topic.md"

# concurrent writer lost the race -> 412 + current_revision
```

## Error model

All errors are `{"error": {"code", "message", "request_id", ...}}` with
`X-Request-ID` echoed. Codes you should handle: `REVISION_CONFLICT` (412,
`current_revision` field), `NOT_FOUND` (404), `PRECONDITION_REQUIRED` (428,
you forgot If-Match), `GONE` (410, trashed), `INVALID_ARGUMENT` (400, e.g.
tampered cursor), `UNAUTHENTICATED` (401), `FORBIDDEN` (403, scope gap),
`TOO_LARGE` (413/422).

## Deployment (see runbook for the full checklist)

- Local: `brainstore-server --data-dir DIR --token T --web-dist apps/web/dist`
  — first boot seeds four template notes (README, usage guide, AI guide,
  inbox) into an empty vault.
- Cloud (Cloudflare Workers + R2 + D1): `git tag v*` publishes binaries and
  the worker bundle (GitHub Release); the `Deploy` workflow ships the worker
  when repo secrets `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` are set.
  Push template content into a fresh cloud vault with:
  `brainstore-cli --profile <cloud> seed`.

## Where this file lives in the vault

A condensed copy of the etiquette above ships as `docs/ai-guide.md` inside
every seeded vault, so an agent that can read the vault can discover its own
rules without leaving the store.
