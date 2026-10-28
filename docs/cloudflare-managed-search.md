# Enable managed search with manual refresh

Use this procedure to add semantic retrieval to an existing Brains Worker while keeping publication under your control. R2 remains authoritative for content and revisions; D1 owns lexical search and the current-source projection. AI Search receives only searchable current sources. It manages its own vector index, so this setup does not create a standalone Vectorize index.

Complete [core deployment](cloudflare.md) first. Keep your `.env`, source notes, resource settings and calibration receipts private. The repository-owned [Cloudflare setup skill](../agents/skills/brains-cloudflare/SKILL.md) points agents to this procedure.

## Check provider usage and budgets

Review the current [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/) and [limits](https://developers.cloudflare.com/d1/platform/limits/). The dated Free profile includes 5 million rows read per UTC day, 100,000 rows written per UTC day, 5 GB of account storage, 10 databases and a 500 MB per-database ceiling. Its default application budgets are 80% of allowances. Counts and account storage include other databases in the same account; a target database's size is a separate observation.

The admission ledger counts application work conservatively. Its totals can exceed Cloudflare's observed rows, particularly after imports. Calibrating those counters is an explicit operator action:

```sh
# Preview account Analytics, database inventory and the proposed baseline.
python3 scripts/cloudflare.py usage

# Capture a fresh observation and retain the original counters in a private receipt.
python3 scripts/cloudflare.py usage --apply \
  --receipt "$HOME/.brains/backups/d1-calibration-$(date -u +%Y%m%dT%H%M%SZ).json"
```

The API token needs Account Analytics Read, D1 Read and D1 Edit permissions. The command captures the ledger before fetching Analytics, compares that same baseline during the conditional update, and preserves all reservations. A concurrent application request invalidates the calibration instead of losing its consumption. Reusing a stale snapshot cannot erase later consumption. The receipt includes both the account-wide provider observation and the previous application counters. Analytics may lag; subsequent application charges remain conservative estimates, so the resulting counters stay labeled estimated. This command is not a background poller or an account-wide billing cap.

An authorized Cloudflare tool can supply `--snapshot provider-observation.json` when the deployment token cannot read Analytics. The snapshot must include the configured account/database, current UTC day, a recent `observed_at`, provider totals, database inventory, and the actual ledger `baseline` captured before that observation. Do not substitute rounded dashboard figures or construct baseline counters from memory. A receipt marked `unconfirmed` means the operation outcome has not been confirmed, for example after a lost SQL response; inspect the live counters before taking another action. Inventory collection targets Free accounts and reads at most 100 databases.

## Configure the managed instance

Create a private `secrets/cloud-services.json`:

```json
{
  "document_conversion": false,
  "url_import": false,
  "semantic": {
    "instance": "brains-notes",
    "publication": "manual",
    "embedding_model": "@cf/baai/bge-m3",
    "dimensions": 1024,
    "ranking": {
      "candidate_limit": 50,
      "validation_limit": 8,
      "result_limit": 5,
      "minimum_score": null
    }
  }
}
```

BGE-M3 is a supported Workers AI model with fixed 1,024-dimensional vectors in managed AI Search. The provisioning helper uses 512-token chunks and zero overlap. Other catalogued models remain configurable; managed dimensions must match the provider catalog. Google models require a separately configured external provider and budget. See [supported models](https://developers.cloudflare.com/ai-search/configuration/models/supported-models/) and [chunking](https://developers.cloudflare.com/ai-search/configuration/indexing/chunking/).

Set `SEMANTIC_SEARCH=on`, `BRAINS_CLOUD_CONFIG_FILE=secrets/cloud-services.json`, `BRAINS_RESOURCE_PROFILE_FILE` to your private reviewed resource profile, `BRAINS_AI_SEARCH_NAMESPACE=default`, and `BRAINS_AI_GATEWAY_ID=brains-notes` in `.env`. Provision and deploy:

```sh
pnpm cloudflare:provision
pnpm cloudflare:check
pnpm cloudflare:deploy
```

Provisioning needs AI Search and AI Gateway management permissions. An authorized Cloudflare MCP can create the same reviewed resources when the deployment token has narrower permissions. The helper creates a dedicated upload-only, vector-only instance and Gateway, disables public search, payload logging, caching, query rewriting and reranking, and uploads no notes. Do not attach the R2 revision bucket as a provider data source: that would also index old revisions. The manual service configuration deploys no reconciliation cron.

AI Search includes 5 million ingestion tokens, 10 GB-month of managed storage, 1,000 semantic queries and 1,000 full-text queries per month. Usage above those allowances becomes billable on November 1, 2026; these are included amounts, not a provider-enforced spending stop. Workers AI embedding and reranking performed by managed AI Search are included in its pricing. Review the latest [AI Search pricing](https://developers.cloudflare.com/ai-search/platform/limits-pricing/) and [billing window](https://developers.cloudflare.com/billing/manage/billable-usage/) before enabling it. At the default 80%, Brains admits at most 800 managed semantic query attempts in the configured window. Other callers of the same account or instance remain outside that counter.

## Backfill once, then refresh on demand

In **Settings → Usage & allowances**, set the actual R2 and AI Search billing-window bounds and enable enforcement. Saved D1 settings override deployment bootstrap settings, so check this page even when your local profile is correct. Billing windows do not renew automatically: expired windows block protected operations until the operator supplies the next verified, non-overlapping period.

Managed ingestion is an opaque charge. The default `search-ingestion` rule has `when_charge_unknown: "block"` and stops before uploading. To approve one controlled backfill, temporarily set that rule's unknown-charge policy to `observe`, leaving query budgets and the other rules enforced. Review the source count and size before doing so. Run:

```sh
python3 scripts/cloudflare.py refresh-search \
  --max-pages 64 --max-steps 120 --timeout 900
```

The command drives the existing two-source reindex pages, then explicitly reconciles at most two jobs per request. Page, request and elapsed-time bounds are independent. It reports queue states and stops on paused work, errors or an exhausted bound; it never schedules an automatic continuation. `ready` confirms completed provider indexing; an accepted upload alone is not completion. If a provider operation fails, inspect its configuration and error before explicitly retrying. Restore `search-ingestion` to `block` after the backfill, including when the command fails. The ingestion counter is a byte-derived estimate, not the provider's final billable token count.

With `publication: "manual"`, new or updated notes remain pending until the next explicit refresh. Normal writes and scheduled invocations can remove obsolete sources but cannot upload new ones. Search checks each candidate's current revision and the caller's permissions; it omits obsolete snippets while a newer note is waiting for refresh. Previously indexed valid notes remain searchable. Switching to `automatic` explicitly enables mutation/cron publication, subject to the same quota policy.

For progress without processing a job, a whole-vault administrator can send `POST /api/v1/maintenance/semantic/reconcile` with `{"limit":0}`. Limits 1 and 2 process bounded work; larger values return 400. The response contains `processed` and per-state counts. Use those states to distinguish `pending`, `queued`, `ready`, `blocked`, `failed`, `deleting` and `delete_failed`; a processed attempt can still finish in a paused state. Polls wait at least 30 seconds between normal attempts; an interrupted claimed job becomes eligible after its 90-second lease plus that delay.

## Verify the deployed behavior

Confirm authenticated capabilities report the configured model and fixed dimensions. Test lexical and hybrid queries in English and in the languages of your notes, and check that hybrid results include semantic retrieval without returning unauthorized or obsolete revisions. Recheck resource usage, retained reservations, the disabled public endpoint and the empty cron list. Storage catalog values do not mean Brains continuously meters the provider's managed GB-month usage.

File navigation reads one raw snapshot containing text, media type and the revision ETag used for conditional saves. Opening the visible properties panel fetches secondary metadata and links; expanding History fetches one revision page, and **Load more revisions** fetches the next page. Hidden mobile properties and collapsed history make no requests. Measure actual request waterfalls as well as elapsed time, since network latency and cold starts vary.

Local repeatable verification uses real isolated workerd D1/R2 with a fake managed provider, plus browser scenarios against the native HTTP server. These checks prove orchestration and UI behavior; validate actual indexing and retrieval separately against your deployed instance:

```sh
python3 tests/cloudflare-usage.py
python3 tests/cloudflare-resources.py
python3 scripts/run-workerd.py --entrypoint tests/fixtures/cloudflare-cost-worker.mjs \
  -- python3 tests/cloudflare-cost-workflows.py
pnpm --dir tests/e2e test specs/file_loading.spec.ts specs/conflict.spec.ts specs/navigation_tags.spec.ts
```
