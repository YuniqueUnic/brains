---
name: brains-cloudflare
description: Set up, deploy or operate this repository's Brains Cloudflare Worker, including dedicated R2/D1 resources, dotenv and GitHub configuration, account D1 usage calibration, and managed AI Search with controlled manual refresh. Use for this product's deployment and quota procedures; consult current provider documentation before changing free-tier assumptions.
---

# Brains Cloudflare operations

Start with [the deployment guide](../../../docs/cloudflare.md), then use [managed search and usage calibration](../../../docs/cloudflare-managed-search.md) for search activation or D1 usage correction. Read [quota semantics](../../../docs/cloudflare-quotas.md) and [cost amplification controls](../../../docs/cloudflare-cost-safety.md) before changing admission policy or background work.

## Establish the target

- Confirm the actual checkout, branch, dirty work, dedicated Worker/R2/D1 names, account and URL. One D1 database serves one vault; changing `VAULT_ID` alone does not isolate shared D1 indexes.
- Read the ignored `.env` locally through `scripts/cloudflare.py`; never print its values or upload the entire file. Preserve the admin registry, session key and cursor key across deployments.
- Reuse the user's existing authorization. Fetch current pricing, limits and resource configuration before choosing budgets; the dated Free catalog is a starting point, not a live account entitlement check.

## Deploy and configure

Use the shared `cloudflare.py` commands for local checks/deployment and GitHub configuration. Keep file and inline JSON inputs mutually exclusive. Deployment bootstrap settings and saved D1 settings are separate: saved settings take precedence, and updates require their current revision.

For semantic retrieval, select a dedicated upload-only managed AI Search instance and its reviewed model/dimensions. The standard manual configuration uses BGE-M3/1024, vector-only indexing, 512-token chunks, zero overlap and no public endpoint. Verify both the instance and Gateway after provisioning. This provider manages its vectors; it needs no independent Vectorize index. Keep conversion, URL import, rewriting and answer generation off unless they are requested and configured.

## Respect usage evidence

Use `cloudflare.py usage` to preview account-wide D1 observations. Use `usage --apply --receipt <private-new-file>` only for an explicit calibration. Preserve reservations and the original counters, reject concurrent or mixed-day baselines, and treat an unconfirmed receipt as an unknown write outcome. Analytics can lag; an observation does not turn future application estimates into invoice-exact counts. Do not clear counters or inflate provider allowances to make an exhausted budget disappear.

Configure verified UTC billing windows and the reviewed budget fraction before enabling enforcement. Keep provider allowances, application budgets, platform ceilings and observational values distinct. The 80% default is user-editable and does not cover other callers or opaque provider work. Explain any unverified billing-window assumption instead of presenting it as provider evidence.

## Refresh explicitly

In manual mode, new source revisions queue without automatic upload. Approve a bounded initial source set, temporarily observe opaque ingestion only for that operation, then drive `cloudflare.py refresh-search` with page, step and time bounds. Inspect returned states until expected sources are `ready`; stop and investigate paused work. Restore the ingestion block after success or failure. Keep the manual mode and empty cron list after a one-time backfill.

Test actual provider retrieval separately from fake-provider orchestration. Verify English and relevant non-English queries, current-revision checks, access scope, budgets/reservations, public endpoint status and cron configuration. Do not publish note bodies or credentials in verification evidence.

Report code checks, isolated runtime checks, actual cloud indexing and actual retrieval as separate results. Link the final PR/CI/deployment and state remaining limitations precisely.
