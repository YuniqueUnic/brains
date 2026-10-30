# D1 ledger receipt conservation

D1 application and bookkeeping statements record their returned `meta.rows_read` and `meta.rows_written`. The next ledger write carries unacknowledged bookkeeping receipts once. [PR #65](https://github.com/YuniqueUnic/brains/pull/65) removed the original per-call allowance; this follow-up removes the remaining worst-case closing allowance from requests whose counter rows are already known to exist.

## Closing paths

A successful debit returns the keys it materialized. The D1 store remembers those keys for the request lifetime, after metadata and result validation. A successful closing batch also establishes its keys. Final accounting resolves its window keys once, then selects one of two paths:

| Evidence for the two resolved keys | Closing SQL | Closing bound |
| --- | --- | --- |
| Both were successfully materialized | Two primary-key UPDATEs of `consumed` and `evidence` | 2 reads / 2 writes |
| Either key is unknown | Two point upserts | 4 reads / 10 writes |

The UPDATE touches no indexed column and has no table trigger. It avoids the rule-index rewrite performed by the upsert. The fallback covers fresh and partially existing rows, rejected admissions and work that has not successfully materialized both keys. Old-window keys do not prove new-window existence. Proof uses existing returned results, without another query, table or shared global cache. Native stores retain their zero-cost default.

The closing batch records pending receipts plus its own bound without recursively metering itself. Acknowledgement covers only the pre-write receipt snapshot and that batch's measured cost; concurrent receipts remain pending. An idle repeated flush issues no SQL. Evidence remains bounded because the final SQL cost is selected before that batch returns, even when the measured cost equals the selected bound.

## Local before and after

Baseline source is `187a53f3a4981a100c1af349334f5f1e867000fe` (0.8.3), rebuilt before the comparison. Both versions use macOS arm64, Rust 1.96.0, Node 24.22.1, pnpm 10.28.2, Wrangler 4.151.0 and worker-build 0.8.7, with compatibility date `2026-10-19`. Each run starts fresh isolated local workerd data with real D1/R2 bindings. Only managed AI Search is a deterministic no-network fake. The dated resource catalog and 80% policy are unchanged; required disposable billing dates are supplied through the settings API.

The first workload has 26 requests: three creates, three updates, six raw reads, six index listings, four FTS searches, three hybrid searches and one resources report. Subsequent lightweight journeys use those same three notes and run before synthetic probes. The browser journey performs session lookup, a store listing, capabilities, raw file read, resources and one resources refresh.

Every table cell is **ledger delta / sum of returned D1 metadata**:

| Workload | Before reads | After reads | Before writes | After writes |
| --- | --- | --- | --- | --- |
| Mixed workload, 26 requests | 4,718 / 4,654 | 4,653 / 4,653 | 1,316 / 1,124 | 1,060 / 1,060 |
| Resources report, 10 requests | 340 / 320 | 320 / 320 | 160 / 100 | 80 / 80 |
| Capabilities, 10 requests | 410 / 390 | 390 / 390 | 160 / 100 | 80 / 80 |
| Raw file, 10 requests | 1,290 / 1,270 | 1,270 / 1,270 | 340 / 280 | 260 / 260 |
| Browser journey, 6 requests | 363 / 351 | 351 / 351 | 126 / 90 | 78 / 78 |

The mixed workload's write ratio improves from 1.1708x to 1.0000x; the lightweight resource-report ratio improves from 1.6000x to 1.0000x. All listed after paths have no surplus over their returned metadata. Time-dependent queue predicates can change read counts slightly between runs; each run must independently conserve its receipts.

The mixed workload closes 32 scopes: 26 requests and six bounded mutation-triggered background scopes. Their UPDATE batches cost 64 reads and 64 writes, exactly the included closing amount. This is not a universal exactness claim: the rejected-admission journey still measures ledger/actual 28/26 reads and 10/4 writes, while a simulated scheduled-query failure measures 18/16 reads and 10/4 writes. Those scopes lack successful key proofs and retain the conservative upsert path.

Separate production-SQL probes run after every business journey, with all real migration indexes installed:

| Two-statement batch | Actual reads | Actual writes | Additional check |
| --- | --- | --- | --- |
| Upsert, both keys fresh | 0 | 10 | Within fallback bound |
| Upsert, both keys existing | 2 | 4 | Within fallback bound |
| Upsert, either one key existing | 1 | 7 | Both directions tested |
| UPDATE, both keys proven | 2 | 2 | Matches known-row bound |
| UPDATE, both keys holding reservations | 2 | 2 | Reserved total remains 6 |

A fresh row writes its table and primary-key, meter, reset and rule indexes. A fresh zero-reservation row does not enter the held partial index. UPDATE modifies neither an index input nor reserved amounts. Future SQL, index, trigger or relevant runtime changes must remeasure these bounds.

## Reproduce and inspect

```bash
pnpm --dir apps/web build
(cd apps/worker && RUSTFLAGS='--cfg getrandom_backend="wasm_js"' worker-build --release)
python3 scripts/run-workerd.py --entrypoint tests/fixtures/d1-ledger-worker.mjs -- \
  python3 tests/d1-ledger-accounting.py --record /tmp/d1-ledger-after.json
python3 scripts/run-workerd.py -- bash tests/worker-smoke.sh
```

For the before measurement, build baseline production source with the same fixture and workload, adding `--baseline --record /tmp/d1-ledger-before.json`. Keep each run's isolated data separate. The runner generates its own configuration and local credentials, applies `wrangler d1 migrations apply BRAINS_INDEX --local`, starts `wrangler dev --local` and cleans up its processes. It strips cloud credentials and does not load the production dotenv file. Never point the fixture at a deployed Worker or cloud profile.

The fixture intercepts every application and bookkeeping D1 result exactly once, including batch results. The simulator supplies usable integer metadata; no fake D1 costs are substituted. It waits for bounded background work and inspects settled ledger totals outside the measured SQL path. Setup, migrations and inspection SQL are excluded from both deltas; no usage table is cleared. The normal resources response is also captured, before that request's own final flush.

For each meter, the test requires `ledger delta = all statement metadata - closing statement metadata + sum(selected closing bounds)`, and separately requires `ledger delta >= all metadata`. Lightweight normal paths additionally require equality. The fixed mixed workload retains its upper bound of twice actual usage. The JSON record includes the fixed workload, all journey statements/deltas and closing probes. Raw SQL is retained locally without bound arguments or credentials.

Unit tests use known-cost stores for sequential/concurrent receipts, closing-time arrivals, repeated flushes, zero checks, rejected admission, reservation release, partial key proofs, new-window keys and native defaults. Runtime journeys cover normal reads, browser flow, 404, stale writes, settings conflicts/save, 429 and scheduled success/failure.

## Operational boundaries

These are local completed-scope measurements, not a real Cloudflare D1 billing comparison or production load test. A dashboard snapshot cannot by itself isolate duplicate accounting: align the UTC window, time cutoff, database/producer scope and metric. Cloudflare account summaries include all databases; this application ledger does not import them. See [dashboard comparison](cloudflare-quotas.md#compare-with-cloudflare-usage) and [Cloudflare D1 metrics](https://developers.cloudflare.com/d1/observability/metrics-analytics/).

No schema or data migration is needed. Existing inflated totals remain until their original windows reset. No limits, rules, fraction or usage tables change. Already-completed I/O can exceed an application cap slightly before later admission rejects work; the implementation neither raises the cap nor retries provider attempts to conceal that result.

Pre-existing gaps remain outside this change: initial state construction can fail before a budget exists; cancellation, final D1 failure or background termination can prevent finalization; pending receipts crossing UTC rollover are assigned to the flush window; the application-query adapter still treats missing metadata as unmeasured/zero. No new account-wide billing guarantee follows from the successful local cases.
