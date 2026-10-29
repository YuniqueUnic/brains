# D1 ledger accounting measurement

## Workload and environment

The baseline uses production source at `e606d223324c39498de243ea61c4dddd3d72e17f`. Both runs build the release Worker and execute the same 26 requests in a fresh isolated local workerd instance: three file creates, three updates, six raw reads, six index listings, four FTS searches, three hybrid searches and one resources report. The dated resource profile and its 80% policy are unchanged. Only the disposable instance's required billing dates are supplied through the settings API.

The environment is macOS arm64, Rust 1.96.0, Node 24.22.1, pnpm 10.28.2, Wrangler 4.151.0 and worker-build 0.8.7, with Worker compatibility date `2026-10-19`. R2 and D1 use real local workerd bindings. Managed AI Search is a deterministic no-network fake; hybrid queries still execute the real D1 reservation and settlement statements.

`tests/fixtures/d1-ledger-worker.mjs` intercepts every application and bookkeeping D1 result once, including each result of a batch. The simulator returns usable integer `meta.rows_read` and `meta.rows_written`; these measurements do not substitute fake D1 costs. The wrapper waits for bounded background work before returning, so the comparison covers completed foreground and background scopes. An uninstrumented inspection query reads the settled ledger before and after the workload. Inspection, fixture setup and migrations are outside both deltas; usage tables are never cleared. The workload also calls the real `GET /api/v1/resources`, whose response is a snapshot before that request's final flush.

## Results

| Meter | Before: ledger / D1 metadata | Before ratio | After: ledger / D1 metadata | After ratio |
| --- | --- | --- | --- | --- |
| Rows written | 6,237 / 1,202 | 5.1889x | 1,316 / 1,124 | 1.1708x |
| Rows read | 12,441 / 5,270 | 2.3607x | 4,716 / 4,652 | 1.0138x |

The before workload executes 311 D1 statements. Baseline admission statements cost 20–31 reads and 4–9 writes each; the reservation batch's three statements cost 5–6 reads / 6–9 writes, 7 reads / 2 writes and 10 reads / 12 writes respectively. Settlement costs 23–24 reads and four writes. Exact metadata now covers those varying costs instead of adding 64 reads and 32 writes on every call.

The after workload closes 32 scopes: 26 requests and six mutation-triggered background scopes. Its only over-count is the final batch bound: 64 extra reads and 192 extra writes across those 32 batches. For each meter, the regression asserts `ledger delta = sum(all statement metadata) - sum(final batch metadata) + closed scopes × final batch bound`. It also requires `actual <= ledger <= 2 × actual` for this fixed workload. Read totals can vary slightly with SQLite timing-dependent predicates; conservation and the stated ratio bound must hold on every run.

The final batch consists of two direct primary-key upserts with no scan or trigger. Separate probes execute the production closing SQL after the workload, against fresh and then existing keys, with all real migration indexes installed:

| Two-statement closing batch | Actual reads | Actual writes | Bound used |
| --- | --- | --- | --- |
| Fresh keys | 0 | 10 | 4 reads / 10 writes |
| Existing keys | 2 | 4 | 4 reads / 10 writes |

A fresh row writes the table plus its primary-key, meter, reset and rule indexes; the held-reservation partial index excludes a fresh zero-reservation row. Two reads per point upsert leave a small lookup/conflict margin. The bound applies once per completed scope and the batch does not recursively meter itself. Repeated idle flushes issue no SQL. Any future change to these statements, indexes or triggers must remeasure this bound; CI exercises the fresh and existing paths.

## Reproduce

Build the web assets once if they are absent, then use the documented local simulation path. `scripts/run-workerd.py` generates an isolated configuration, applies `wrangler d1 migrations apply BRAINS_INDEX --local`, starts `wrangler dev --local`, supplies random local-only credentials and cleans up the processes. It strips cloud credentials and does not load the production dotenv file.

```bash
pnpm --dir apps/web build
(cd apps/worker && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release)
python3 scripts/run-workerd.py --entrypoint tests/fixtures/d1-ledger-worker.mjs -- \
  python3 tests/d1-ledger-accounting.py --record /tmp/d1-ledger-after.json
python3 scripts/run-workerd.py -- bash tests/worker-smoke.sh
```

For a baseline comparison, build the baseline production source with the same fixture and use `--baseline --record /tmp/d1-ledger-before.json`. Keep each run's isolated data separate. Do not point this fixture or workload at a deployed Worker or a `cloud` CLI profile. Raw evidence contains SQL text but no bound arguments or credentials; it remains a local artifact.

The harness also checks read-only zero admissions, 404 responses, stale file writes, conflicting and successful settings updates, rejected request admission, scheduled reconciliation and a simulated reconciliation-query failure. Each completed journey must conserve all returned D1 receipts. Application unit tests use a store with known receipt costs to cover one final bound, concurrent receipts and acknowledgements, repeated flushes, rejected admissions, reservation release and native zero-cost defaults.

## Limits and operational effect

These results establish the local accounting relationship for the measured SQL and completed scopes. They do not measure real Cloudflare D1, production traffic, provider-wide usage or invoices. Cloudflare statement metadata and analytics remain the provider source of truth; account dashboards aggregate other databases and clients. See [dashboard comparison](cloudflare-quotas.md#compare-with-cloudflare-usage) and [Cloudflare's D1 row accounting](https://developers.cloudflare.com/d1/platform/pricing/).

There is no schema or data migration. Existing inflated totals remain until their original windows reset. No usage table, resource rule, limit or 80% fraction changes. Observing already-completed work can exceed an application cap by its measured cost and closing bound; it does not authorize new work or repeat a provider attempt. Unknown-charge and exhaustion policies, atomic multi-meter admission and release-only-uncharged settlement remain in place.

Pre-existing gaps remain outside this fix: state construction can fail after its initial settings query and before a budget exists; runtime cancellation, a failed final D1 write or termination of `waitUntil` work can prevent completion; receipts pending across a UTC boundary are assigned to the flush window. The local fixture intentionally waits for background completion and does not prove durable accounting under those failures. The ledger is application admission control, not an account-wide zero-overage guarantee.
