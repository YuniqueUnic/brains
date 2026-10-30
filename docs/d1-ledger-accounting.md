# D1 ledger receipt conservation

D1 application and bookkeeping statements record their returned `meta.rows_read` and `meta.rows_written`. The next ledger write carries unacknowledged bookkeeping receipts once. [PR #65](https://github.com/YuniqueUnic/brains/pull/65) removed the original per-call allowance; this follow-up removes the remaining worst-case closing allowance from requests whose counter rows are already known to exist.

## Closing paths

A successful debit returns the keys it materialized. The D1 store remembers those keys for the request lifetime, after metadata and result validation. A successful closing batch also establishes its keys. Final accounting resolves each window key once, then chooses its statement independently:

| Evidence for one resolved key | Closing SQL | Per-statement closing bound |
| --- | --- | --- |
| Successfully materialized | Primary-key UPDATE of `consumed` and `evidence` | 1 read / 1 write |
| Not yet proven | Point upsert | 2 reads / 5 writes |

The UPDATE touches no indexed column and has no table trigger. It avoids the rule-index rewrite performed by the upsert. Two known keys therefore close at 2 reads / 2 writes; two unknown keys use 4 reads / 10 writes. A mixed pair uses one of each. Old-window keys do not prove new-window existence. Proof uses existing returned results, without another query, table or shared global cache. Native stores retain their zero-cost default.

The closing batch records pending receipts plus its own bound without recursively metering itself. Acknowledgement covers only the pre-write receipt snapshot and that batch's measured cost; concurrent receipts remain pending. An idle repeated flush issues no SQL. Evidence remains bounded because the final SQL cost is selected before that batch returns, even when the measured cost equals the selected bound.

Raw bookkeeping receipts carry the operation-start UTC timestamp, including settings reads before the final budget is constructed. A close spanning midnight writes both windows, and adds the entire closing batch's bound only to the window in which that batch starts. Historical receipts, including zero-amount Unknown markers, are observed before a new window's admission. Expired zero-amount guards do not change settlement of a still-valid provider reservation.

## Failure and cancellation behavior

Application SQL must return both row counts; zero is a valid measurement, absence is not. Missing metadata records Unknown evidence and returns an error. With strict `when_charge_unknown`, new controlled work is blocked even if `when_exhausted` is set to observe; administrator recovery endpoints remain available. Completed business operations are never replayed to recover metadata. A successful terminal batch with missing result metadata uses its already-written closing bound and acknowledges all available closing receipts, without issuing another batch.

After a ledger write is dispatched, a lost response cannot prove whether it committed. Input receipts are not replayed. A failed final batch returns 503 and leaves an unresolved state for the request; subsequent flushes of that same budget continue to fail rather than becoming idle successes. This state is request-local. If D1 refuses the write, the application cannot promise to persist its missing receipts or an Unknown marker. Durable exactly-once recovery across an unavailable store would require additional durable coordination and is not claimed here.

The request driver is registered with `waitUntil` before its first database await, independently of the response consumer. Recoverable settings-load, optional-service construction and router exits run final accounting while the budget is still owned. Disconnecting a client does not replay its business operation. Cloudflare can still cancel remaining work after the `waitUntil` limit, or terminate an invocation for a resource/runtime failure; no in-process cleanup can persist receipts after that termination. [Worker execution lifetime](https://developers.cloudflare.com/workers/runtime-apis/context/#waituntil).

## Local before and after

Baseline source is `187a53f3a4981a100c1af349334f5f1e867000fe` (0.8.3), rebuilt before the comparison. Both versions use macOS arm64, Rust 1.96.0, Node 24.22.1, pnpm 10.28.2, Wrangler 4.151.0 and worker-build 0.8.7, with compatibility date `2026-10-19`. Each run starts fresh isolated local workerd data with real D1/R2 bindings. Only managed AI Search is a deterministic no-network fake. The dated resource catalog and 80% policy are unchanged; required disposable billing dates are supplied through the settings API.

The first workload has 26 requests: three creates, three updates, six raw reads, six index listings, four FTS searches, three hybrid searches and one resources report. Subsequent lightweight journeys use those same three notes and run before synthetic probes. The browser journey performs session lookup, a store listing, capabilities, raw file read, resources and one resources refresh.

Every table cell is **ledger delta / sum of returned D1 metadata**:

| Workload | Before reads | After reads | Before writes | After writes |
| --- | --- | --- | --- | --- |
| Mixed workload, 26 requests | 4,718 / 4,654 | 4,408 / 4,408 | 1,316 / 1,124 | 1,060 / 1,060 |
| Resources report, 10 requests | 340 / 320 | 320 / 320 | 160 / 100 | 80 / 80 |
| Capabilities, 10 requests | 410 / 390 | 390 / 390 | 160 / 100 | 80 / 80 |
| Raw file, 10 requests | 1,290 / 1,270 | 1,150 / 1,150 | 340 / 280 | 260 / 260 |
| Browser journey, 6 requests | 363 / 351 | 330 / 330 | 126 / 90 | 78 / 78 |

The mixed workload's write ratio improves from 1.1708x to 1.0000x; the lightweight resource-report ratio improves from 1.6000x to 1.0000x. All listed after paths have no surplus over their returned metadata. SQL admission changes and time-dependent queue predicates can change read costs; each run must independently conserve its receipts rather than compare raw read counts alone.

The mixed workload closes 32 scopes: 26 requests and six bounded mutation-triggered background scopes. Their UPDATE batches cost 64 reads and 64 writes, exactly the included closing amount. This is not a universal exactness claim: the rejected-admission journey still measures ledger/actual 25/23 reads and 10/4 writes. A simulated scheduled-query failure records Unknown evidence while conserving its measured bookkeeping: 30/30 reads and 6/6 writes. Other scopes without successful key proofs retain the conservative upsert path.

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
python3 scripts/run-workerd.py --entrypoint tests/fixtures/d1-ledger-worker.mjs -- \
  python3 tests/d1-ledger-failures.py
python3 tests/cloudflare-observations.py
python3 scripts/run-workerd.py -- bash tests/worker-smoke.sh
```

For the before measurement, build baseline production source with the same fixture and workload, adding `--baseline --record /tmp/d1-ledger-before.json`. Keep each run's isolated data separate. The runner generates its own configuration and local credentials, applies `wrangler d1 migrations apply BRAINS_INDEX --local`, starts `wrangler dev --local` and cleans up its processes. It strips cloud credentials and does not load the production dotenv file. Never point the fixture at a deployed Worker or cloud profile.

The fixture intercepts every application and bookkeeping D1 result exactly once, including batch results. The simulator supplies usable integer metadata; no fake D1 costs are substituted. It waits for bounded background work and inspects settled ledger totals outside the measured SQL path. Setup, migrations and inspection SQL are excluded from both deltas; no usage table is cleared. The normal resources response is also captured, before that request's own final flush.

For each meter, the test requires `ledger delta = all statement metadata - closing statement metadata + sum(selected closing bounds)`, and separately requires `ledger delta >= all metadata`. Lightweight normal paths additionally require equality. The fixed mixed workload retains its upper bound of twice actual usage. The JSON record includes the fixed workload, all journey statements/deltas and closing probes. Raw SQL is retained locally without bound arguments or credentials.

Unit tests use known-cost stores for sequential/concurrent receipts, closing-time arrivals, repeated flushes, zero checks, rejected admission, reservation release, partial key proofs, old/new-window allocation, unknown-only historical receipts, ambiguous commits, partial closing metadata and native defaults. Runtime journeys cover normal reads, browser flow, 404, stale writes, settings conflicts/save, 429, scheduled work, failed construction, missing application/closing metadata, final-write failure and a real client disconnect between SQL commit and receipt return. The fault fixture records real metadata before selectively hiding it from the application; it does not substitute invented D1 costs.

Browser scenarios in `resource_loading.spec.ts` verify desktop/mobile first-visit loading, unsaved drafts across tabs, explicit refresh, late responses after closing and localized fresh/expired sample presentation. Native profiles verify that unsupported resource controls make no resource request. The browser sample renderer uses a labeled presentation fixture over the real API response; account aggregation is separately tested against the offline inventory client and real workerd configuration path.

## Operational boundaries

These are local completed-scope measurements, not a real Cloudflare D1 billing comparison or production load test. A dashboard snapshot cannot by itself isolate duplicate accounting: align the UTC window, time cutoff, database/producer scope and metric. Cloudflare account summaries include all databases; this application ledger does not import them. See [dashboard comparison](cloudflare-quotas.md#compare-with-cloudflare-usage) and [Cloudflare D1 metrics](https://developers.cloudflare.com/d1/observability/metrics-analytics/).

No schema or data migration is needed. Existing inflated totals remain until their original windows reset. No limits, rules, fraction or usage tables change. Already-completed I/O can exceed an application cap slightly before later admission rejects work; the implementation neither raises the cap nor retries provider attempts to conceal that result.

Production D1 row accounting and arbitrary production traffic were not benchmarked. A separately authorized account Analytics read confirmed that the owner-provided screenshots and the later aggregate were different cutoffs; it does not establish an exact dashboard lag or reconcile them with a matching settled ledger snapshot. No account-wide billing guarantee follows from completed local cases or from a timestamped storage observation.
