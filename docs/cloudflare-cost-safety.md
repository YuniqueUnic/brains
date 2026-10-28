# Cloudflare cost amplification and Brains controls

Reviewed on 2026-10-28. This review traces how one request, scheduled event or failed operation can create more billed work. The application ledger controls operations routed through Brains; it is not an account-wide spending cap. Recheck current provider pricing and plan limits before deploying or changing the example resource profile.

| Resource | Billed work relevant to this review |
| --- | --- |
| Workers | Incoming invocations, CPU, and enabled observability. A rejected request has already reached the platform. |
| D1 | Rows scanned/written, index maintenance and retained storage; returned row count is insufficient. |
| R2 | Object storage, Class A operations such as listing/writing, and Class B reads. Free egress does not make requests free. |
| KV | Key operations and retained storage. Brains currently has no KV binding. |
| AI Search | Managed ingestion, storage and queries. Background indexing can continue after an upload returns. |

## Reported incidents and the mechanisms they illustrate

These are firsthand public reports, not independently audited invoices. Dollar amounts must not be used as verified pricing examples. Technical conclusions below are checked against provider documentation.

| Report | What the source establishes | Relevant failure mode |
| --- | --- | --- |
| [D1 infinite-write loop](https://www.reddit.com/r/CloudFlare/comments/1r7skeq/support_said_my_48k_billing_dispute_was_on_hold/) | The author reports an overnight charge of about $4,868 from a code loop, followed by a billing dispute and an update saying the amount was waived. The post and its later update use different amounts; no independent invoice verification. | A background write loop can accumulate work while normal traffic is low. Retries need a stopping condition. |
| [Queue calling its own async API](https://www.reddit.com/r/CloudFlare/comments/1t1e8nh/i_accidentally_generated_16_billion_durable/) | The author describes a consumer forwarding `write_mode=async` to the enqueue endpoint and marking the original message complete. The reported totals disagree between the title, body and breakdown; this review does not adopt a reconciled dollar total. | A successful 202 can create a new task rather than finish the original. New messages escape the retry counter of the previous message. |
| [KV cache accumulation](https://meathill.com/posts/cloudflare-worker/nextjs-opennext-cloudflare-kv-storage-leak-guide) | The author traces approximately 24 GB of cache data to omitted expiration and build-specific keys. The article does not establish a final invoice total or the behavior of every current OpenNext release. | Soft freshness and changing build identifiers do not delete persistent keys. Build-time and runtime writes need the same retention rules. |
| [R2 public-domain traffic spike](https://www.reddit.com/r/CloudFlare/comments/1kqunk2/r2_how_did_this_happen/) | The author reports a public custom-domain spike and about $150 paid, while acknowledging expired logs and uncertain details. Operation counts, current unit prices and the reported amount do not independently reconcile the invoice. | Public object requests can produce operation charges even when egress is free. Removing the application API does not protect a separately public bucket. |

D1 measures scanned rows and index-related writes, so frequently repeated unindexed queries can amplify cost even with a small `LIMIT`. Inspect `EXPLAIN QUERY PLAN` and provider `meta.rows_read`/`meta.rows_written`; do not substitute the number of returned records. See [D1 indexes](https://developers.cloudflare.com/d1/best-practices/use-indexes/), [pricing](https://developers.cloudflare.com/d1/platform/pricing/) and [billing analytics](https://developers.cloudflare.com/d1/observability/billing/).

KV expiration must be explicitly supplied; read-cache TTL is a different setting. Cache cleanup must not be applied to source documents or immutable revision history. See [KV writes and expiration](https://developers.cloudflare.com/kv/api/write-key-value-pairs/).

R2 Standard allowances, Infrequent Access storage/retrieval rules and operation charges differ. A conditionally rejected write can still count after authentication. See [R2 pricing and failed requests](https://developers.cloudflare.com/r2/pricing/).

## What platform protection does and does not stop

- [Budget alerts](https://developers.cloudflare.com/billing/manage/budget-alerts/) notify; they do not pause or cap usage. The invoice is authoritative. An alert is not an emergency stop mechanism.
- [Workers loop protection](https://developers.cloudflare.com/workers/observability/errors/#loop-limit) limits a single call chain to 16 Worker hops. A new cron event, queue message or external retry is a new chain.
- [Invocation limits](https://developers.cloudflare.com/workers/platform/limits/#subrequests) bound one invocation. As reviewed, Free allows 50 external and 1,000 internal-service subrequests; Paid defaults to 10,000 and supports higher configured limits. Do not assume the historical Paid limit of 1,000 still applies. [CPU limits](https://developers.cloudflare.com/workers/platform/pricing/#workers) also act per invocation, not per billing period.
- Free-tier hard failures and Paid included allowances have different semantics. In particular, moving to Paid must not be treated as preserving Free D1 daily stop limits. R2 allowances are separate from the Workers plan. Consult each product's current pricing.
- [AI Gateway spend limits](https://developers.cloudflare.com/ai-gateway/features/spend-limits/) can reject covered calls, but use estimated, eventually consistent costs. They do not cap D1/R2/KV or AI Search's included internal embedding/reranking operations. See the [AI Search Gateway boundary](https://developers.cloudflare.com/ai-search/configuration/models/ai-gateway/).

## Findings and controls in Brains

### Authenticate before storage and accounting

The Worker previously loaded resource settings from D1 and wrote a request charge before the shared router authenticated the caller. Public health polling and unauthorized API traffic could therefore generate D1 operations even when they never reached a business handler.

`AuthSource::authorize_request` now owns the shared credential, Origin and CSRF checks. The Worker runs that policy before loading D1/R2 services. `/healthz` is a storage-free liveness response. Shared native middleware uses the same policy. Traffic rejected by this early identity/Origin/CSRF gate and health traffic still consumes platform invocation/CPU capacity, but does not amplify into this application's D1/R2/AI operations. Authenticated requests can still incur setup/accounting before a later route, scope or input rejection.

### Do not let failed requests dispatch background work

The Worker previously inferred a mutation from most non-read methods and scheduled reconciliation regardless of response status. It now schedules only successful, explicitly listed content/maintenance mutations. Search, session/settings changes, unknown routes, denied requests and failed writes do not dispatch semantic work.

Cron handles at most one leased semantic item per invocation. When semantic search is off, it returns before loading storage settings. Optional-service configuration and preview deployments still need review: a deployed cron continues to run without a browser being open.

### Stop failed semantic jobs instead of recreating them

The old reconciler deleted and reuploaded every provider status other than completed/queued/running. A document that the provider permanently rejected or skipped could therefore be ingested repeatedly.

A queued poll that observes provider `error`, `skipped`, `outdated` or an unknown state now stops as local `failed`, retaining the remote item identifier and reason. Other processing errors pause ingestion as `failed`, policy/quota failures as `blocked`, and errors while processing deleting jobs as `delete_failed`. Automatic claims include only pending, queued and deleting work. They do not repeatedly retry failed or blocked work. A normal queued/running item may be polled without recreating it. Provider statuses are defined by the [AI Search Items API](https://developers.cloudflare.com/ai-search/api/items/workers-binding/).

After fixing content, provider configuration or budget policy, run `bsx reindex --all-pages` as a whole-vault administrator. The first page explicitly resumes one retry batch, bounded by that page's item limit, including `delete_failed` jobs for purged paths. Live projections are republished as subsequent pages run. Remaining paused cleanup jobs require another explicit maintenance run; they are not silently drained by cron. A pending explicit retry may remove and recreate one known failed provider item; ordinary queued polling cannot. Unknown provider statuses still pause for investigation. Deletion retry looks up the immutable key again, so an already removed item can finish as deleted. Stopping retries does not itself prove that a provider object was deleted. Source revisions remain intact and retrieval still validates current HEAD and permissions.

### Bound authoritative R2 scans without losing paths

R2 HEAD keys end in `.json`. Their lexical order differs from logical path order, so applying `startAfter(path + '.json')` would skip valid paths. The current authoritative listing sorts a raw key scan, then returns the requested logical page. Reindex's small output pages can repeat that scan.

`R2_LIST_PAGE_MAX` bounds each raw scan to four pages by default, with up to 1,000 keys per page. A truncated scan fails explicitly rather than returning an incomplete page as complete. For queries that accept a prefix, narrow it before raising the configured limit. Whole-vault reindex has no prefix selector: a vault beyond the bound needs an explicitly reviewed higher limit; reducing its output page size does not reduce the raw scan. Missing/nonadvancing R2 cursors fail instead of restarting a scan. This is a bounded scan, not an efficient new storage cursor: large-vault logical pagination remains a separate ordering change that needs migration and correctness evidence.

Zero-result listing no longer writes a charge for zero R2 reads. Actual list attempts remain charged. A failed mutation or R2 conditional write is not automatically free.

### Count the ledger's own work honestly

The active and paused semantic queues have partial indexes matching their state filters and timestamp order. A small returned `LIMIT` alone would not prevent a backlog scan/sort; migrations install these indexes once, outside request handling.

The D1 ledger uses raw D1 operations, not the metered index adapter; it does not recursively meter itself. Its overhead is still real. A raw-file GET can require a settings read, request accounting and separate HEAD/revision/blob accounting.

The current ledger allocates 64 read rows and 32 written rows per accounting operation. These configured accounting allowances are not proven universal provider-cost upper bounds; retained reservations and query/index work can vary. Four such operations can report 128 bounded written rows without proving that Cloudflare billed 128 rows. Keep `bounded`, `estimated`, `unknown` and provider-reported observations distinct. Compare account analytics and D1 result metadata before describing a bill or changing a budget. Do not reset usage or inflate allowances to make an over-budget display disappear.

When enforcement is disabled, the application observes rather than blocks. Saved resource settings override deployment defaults, so changing an example file does not silently enable a previously saved policy. Enable enforcement through the revision-checked settings API after reviewing applicable billing windows and actual usage. It cannot retroactively stop a migration, CLI operation, another Worker or provider-side work outside this application.

Whole-vault administrators can still sign in, inspect capabilities and repair resource settings when the request budget is exhausted. These recovery requests are observed instead of blocked. Other credentials remain subject to the request budget, including on those same paths; possession of a read-only token does not grant a recovery bypass.

## Configuration

The deployment script composes these independent settings; each has one unit and purpose:

```dotenv
# R2 scan bound, independent of the response page size.
BRAINS_R2_LIST_PAGE_MAX=4

# Optional invocation controls: verify your plan's supported range first.
# Empty values preserve platform defaults. These are not dollar limits.
BRAINS_WORKER_CPU_MS=
BRAINS_WORKER_SUBREQUESTS=

# Sample invocation logs; traces retain their independent setting.
BRAINS_WORKER_LOG_SAMPLE_RATE=0.1
```

The tracked log sample rate is 0.1. Temporarily increase it for a specific investigation, then restore the chosen operating level. Sampling reduces log volume; it does not stop request or storage charges. [Workers Logs sampling](https://developers.cloudflare.com/workers/observability/logs/workers-logs/).

Keep the R2 source bucket private, use dedicated deployment resources, retain backups and inspect account-level usage notifications. Do not add KV, queues, Durable Objects or a new retry framework merely to implement these controls. Do not apply cache expiration to version history.

## Verification boundaries

The cost-workflow suite passed against local workerd D1/R2 bindings with a deterministic managed-provider fake. Existing Worker content/history/search smoke and durable quota/settings recovery also passed. Provider invoices and real managed-ingestion charges were not exercised.

Relevant validation must prove that unauthenticated and health traffic leaves the D1 ledger unchanged; failed requests do not schedule semantic work; failed provider items stop instead of reuploading; normal pending work can complete; a scan over its configured bound fails clearly; and authenticated content/history/search still work. Run this against isolated workerd/D1/R2 and the existing shared HTTP tests. Wrangler platform `limits` are enforced on Cloudflare, not by local development; CPU limits also have runtime flexibility. A local simulation is not proof that an invocation cap was enforced, a provider invoice, a managed ingestion test or an account-wide spending guarantee. See [Wrangler limits](https://developers.cloudflare.com/workers/wrangler/configuration/#limits).
