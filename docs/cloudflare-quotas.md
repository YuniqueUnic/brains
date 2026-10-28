# Cloudflare resource rules, usage budgets and model configuration

The Worker loads these resource rules through the shared deployment helper. The [complete example profile](examples/cloudflare-free-2026-10-27.json) contains the October 27, 2026 Free-tier snapshot, with application allowance defaults at 80%. Copy it, fill real billing-cycle bounds and review current provider pricing before use. The catalog includes informational and platform-managed values as well as implemented admission meters; see the enforcement table below.

## Configuration primitives

Represent each quota or constraint with one rule assembled from six small values. Keep configuration separate from runtime `Usage` and `Reservation` records. Do not encode the resource, metric, unit and period in a long property name or infer behavior by splitting a rule ID.

| Primitive | Question answered | Example |
| --- | --- | --- |
| `resource` | Which provider service and resource type is measured? | `{ "service": "d1", "kind": "database" }` |
| `measure` | What is measured, and in which unit? | `{ "metric": "rows_read", "unit": "row" }` |
| `scope` | Which objects share this limit? | `{ "kind": "account" }` |
| `window` | When does the measurement reset or expire? | `{ "kind": "calendar", "size": 1, "unit": "day", "timezone": "UTC" }` |
| `limit` | What does the provider offer or constrain? | `{ "kind": "allowance", "value": 5000000 }` |
| `policy` | How much will this application allow? | `{ "fraction": 0.8 }` or `{ "maximum": 3000000 }` |

A rule has a human-readable `id` for references and an official `source`. The ID is an alias, not an executable expression. Supported service/resource/metric/unit combinations resolve through small provider adapters. Reject unsupported combinations rather than guessing their meaning or treating unknown values as unlimited. Resolve account and resource identities from actual deployment bindings.

`scope` identifies the accounting partition: `account`, `resource`, `invocation` or `record`. `window` independently describes `none`, a calendar interval with a timezone, a real provider billing cycle reference, or a rolling interval. For example, a database capacity limit has resource scope and no renewal window; a per-invocation query limit has invocation scope and no calendar reset. An active-user seat pool is current capacity, not a daily allowance.

`limit.kind` distinguishes `allowance`, `ceiling`, `reference`, `unmetered` and `unknown`. Allowances can be consumed or occupied; ceilings are provider maxima; a reference such as a default timeout is informational; unmetered means the documented operation has no usage charge, while unknown means its entitlement was not verified. Unknown and unmetered are never interchangeable. Retention is measured explicitly as a duration and is not a spendable token pool. An unknown informational retention value does not block an unrelated operation; admission blocks only when that operation needs a protected meter whose required amount or entitlement is unknown.

Apply the default fraction of `0.8` to allowances. Keep ceilings at their documented value unless a rule explicitly requests a lower application watermark. A rule's `policy.maximum` takes precedence over its fraction. Countable values round down in their base unit; fractional storage/time values retain normalized precision, so 80% of 0.5 GB is 0.4 GB rather than zero. A zero effective limit blocks work. A user can deliberately raise an allowance budget above included usage, with the loss of free-tier protection reported; a policy cannot raise a provider ceiling.

## The same rule shape for different resources

D1 reads share an account-wide daily allowance:

```json
{
  "id": "d1-reads",
  "resource": { "service": "d1", "kind": "database" },
  "measure": { "metric": "rows_read", "unit": "row" },
  "scope": { "kind": "account" },
  "window": { "kind": "calendar", "size": 1, "unit": "day", "timezone": "UTC" },
  "limit": { "kind": "allowance", "value": 5000000 }
}
```

The default policy derives 4,000,000 rows/day. A database capacity rule uses the same primitives, with an explicit lower watermark:

```json
{
  "id": "d1-database-storage",
  "resource": { "service": "d1", "kind": "database" },
  "measure": { "metric": "storage", "unit": "MB" },
  "scope": { "kind": "resource" },
  "window": { "kind": "none" },
  "limit": { "kind": "ceiling", "value": 500 },
  "policy": { "fraction": 0.8 }
}
```

This derives a 400 MB application watermark while retaining the real 500 MB provider ceiling. CPU uses metric `cpu`, unit `ms`, invocation scope and no renewal window. Log retention uses metric `retention`, unit `day`, record scope and a duration ceiling. Browser launch rate uses a rolling time window. These require no separate `watermarks`, `retention` or `optional_allowances` schemas.

Rules for optional products use the same list. Activate a resource's rules only when its actual adapter or binding is enabled. In particular, a managed AI Search instance does not activate standalone Vectorize, direct Browser Run or legacy Gateway log resources. Legacy logging eligibility depends on the account's first Gateway creation date and must be verified during setup.

## Coverage and dated defaults

Users must actively review current official pricing, their actual plan, account-sharing scope and billing cycle. The snapshot below is configuration data, not a permanent product entitlement or a claim that the current account uses Free. Changing a review date, rule alias, source value or effective limit must not erase consumed usage. A renamed rule resolves to the same normalized provider meter and window.

| Product | Included/free usage or capacity | Default application amount |
| --- | --- | --- |
| Workers | 100,000 dynamic requests/day. Pure static asset requests are unmetered. | 80,000 dynamic requests/day. |
| R2 Standard | 10 GB-month storage; 1M Class A and 10M Class B operations/month. | 8 GB-month; 800,000 Class A and 8M Class B. |
| D1 Free | 5M rows read/day; 100,000 rows written/day; 5 GB account storage. | 4M reads; 80,000 writes; 4 GB account storage. |
| Workers AI | 10,000 neurons/day. | 8,000 neurons/day. |
| AI Search | Announced monthly inclusion: 5M ingestion tokens, 10 GB-month storage, 1,000 semantic and 1,000 full-text queries. Billing starts November 1, 2026. | 4M tokens; 8 GB-month; 800 queries in each distinct query pool. |
| Workers Logs through November 30, 2026 | 200,000 events/day on Free; 3-day retention. | 160,000 events/day; retention remains a separate duration. |
| Access Free | 50 active-user seats, current capacity. | 40-user watermark. |
| Optional direct Browser Run | 600 browser-seconds/day. | 480 seconds/day. |
| Optional standalone Vectorize | 30M queried dimensions/month; 5M currently stored dimensions. | 24M queried dimensions/month; 4M stored dimensions. |
| Optional eligible legacy Gateway logs | 100,000 stored logs, current capacity. | 80,000-log watermark; no daily renewal. |

Sources: [Workers pricing and limits](https://developers.cloudflare.com/workers/platform/limits/), [R2 pricing](https://developers.cloudflare.com/r2/pricing/), [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/), [Workers AI](https://developers.cloudflare.com/workers-ai/platform/pricing/), [AI Search](https://developers.cloudflare.com/ai-search/platform/limits-pricing/), [Workers Logs](https://developers.cloudflare.com/workers/observability/logs/workers-logs/), [Access seats](https://developers.cloudflare.com/cloudflare-one/team-and-resources/users/seat-management/), [Browser Run](https://developers.cloudflare.com/browser-run/pricing/), [Vectorize](https://developers.cloudflare.com/vectorize/platform/pricing/), [Gateway pricing](https://developers.cloudflare.com/ai-gateway/reference/pricing/).

The complete profile also records material operational ceilings: Workers CPU, memory, outgoing connections, subrequests, deployment assets, Worker/Cron counts; D1 per-database capacity, database count and invocation query count; AI Search instance/file/metadata/candidate limits; MCP portal/server/domain counts; and direct-browser concurrency/rate/default-timeout values. Provider ceilings, observed usage and application-enforced admission remain distinct. An inventory review must add rules whenever a new chargeable dependency is enabled.

From December 1, 2026, announced Free Observability pricing uses one shared 0.5 GB/day ingestion pool for Workers Logs, Workers Traces and AI Gateway logs, with 7-day retention. The 80% budget is 0.4 GB/day for the pool, not for each producer. The profile records this as a future rule replacement requiring review. Do not keep counting both the old event pool and the new byte pool, or silently mutate limits from fetched pricing. Current traces have 7-day retention; no separate verified Free trace-count pool is assumed. [Observability pricing](https://developers.cloudflare.com/observability/pricing/), [Workers traces](https://developers.cloudflare.com/workers/observability/traces/).

MCP tool-log quantity/retention remains unknown where the portal documentation does not specify it; Access authentication-log retention must not be presented as MCP tool-log retention. MCP Logpush requires Enterprise. Gateway core features are unmetered, but associated inference and logging follow their respective meters. [MCP portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/), [Zero Trust logs](https://developers.cloudflare.com/cloudflare-one/insights/logs/).

## Model and retrieval configuration

Default to the Cloudflare-hosted Qwen model through managed AI Search; the current managed dimension is 1,024. Users can choose another supported model and request dimensions, but configuration validation must check what that adapter actually exposes. See [the model/dimension matrix and multimodal boundaries](cloudflare-ai-plan.md#model-and-dimension-policy).

Use [cloudflare-services.json](examples/cloudflare-services.json) for enabled services and ranking, and the complete resource profile for quotas. Set `BRAINS_CLOUD_CONFIG_FILE` and `BRAINS_RESOURCE_PROFILE_FILE` locally; Actions accepts their inline JSON equivalents. These are two explicit inputs, not a profile-inheritance or rule-override language.

For example, edit the full `search-semantic` rule in your copied profile to use this policy:

```json
{
  "id": "search-semantic",
  "resource": {"service": "ai_search", "kind": "instance"},
  "measure": {"metric": "semantic_queries", "unit": "query"},
  "scope": {"kind": "account"},
  "window": {"kind": "billing_cycle", "ref": "ai_search"},
  "limit": {"kind": "allowance", "value": 1000},
  "policy": {"maximum": 600},
  "source": "ai_search"
}
```

The example is one complete rule within the profile, not a standalone replacement for the full catalog. Global policy provides fraction/exhaustion/unknown-charge defaults; a per-rule policy overrides those fields and can set an absolute maximum in that rule's display unit. A global maximum is rejected because the rules use different units. Runtime facts use canonical units, including bytes for measured capacity. Changing a display unit, alias, profile ID or limit does not reset usage.

`context.billing_cycles` contains actual `{ "start": <unix-seconds>, "end": <unix-seconds> }` bounds. A required missing/expired cycle blocks the protected operation. The runtime supports UTC fixed calendar intervals and explicit billing windows; rolling limits remain platform-managed. The `changes` section records dated pricing review items and does not automatically switch entitlements or reset counters.

The helper serializes the full profile into deterministic ASCII chunks below the Worker's 5 KB per-variable limit. The Worker reassembles it at the composition root. Users still edit one profile file.

A null `minimum_score` means no application quality cutoff. The AI Search adapter must explicitly send `ai_search_options.retrieval.match_threshold: 0`, because omitting it leaves the provider default of 0.4. Request at most 50 candidates, then apply source/permission checks, document deduplication, RRF and the result count. The first release leaves a true model reranker off. Authorization and freshness filtering remain mandatory. [Search binding](https://developers.cloudflare.com/ai-search/api/search/workers-binding/).

For native Google Embedding 2, request a supported output size such as 1,024 from its 3,072 native dimensions; the API automatically normalizes that output. Managed AI Search currently exposes Google at 1,536 dimensions without a dimension override. For self-managed Qwen, the model author supports a prefix crop followed by L2 normalization even though Workers AI does not expose a dimension parameter. Unsupported paths reject dimension overrides. Model modalities and accepted adapter inputs are separate: Google supports text/image/audio/video/PDF, while the default Brains path is text. [Google normalization](https://ai.google.dev/gemini-api/docs/embeddings#ensuring-quality-for-smaller-dimensions), [Qwen implementation](https://github.com/QwenLM/Qwen3-Embedding/blob/main/evaluation/qwen3_embedding_model.py).

An optional Google profile uses its own project quotas, provider credentials and billing policy. RPM/TPM/RPD apply per project; RPD resets at Pacific local midnight. Do not derive Google allowances from the Cloudflare snapshot. Google spending controls have accounting latency and are not a synchronous zero-overage guarantee. [Google rate limits](https://ai.google.dev/gemini-api/docs/rate-limits), [spend caps](https://ai.google.dev/gemini-api/docs/billing#spend-caps).

## Runtime usage and admission

The runtime stores two additional small records: `Usage` holds consumed/reserved amounts and evidence for a normalized meter/window; `Reservation` holds one outbound attempt's allocated amounts and delivery/settlement state. These records are not free-tier configuration and must survive redeployment and policy edits.

```text
authorize operation
  -> resolve active resources, meters, units, scopes and windows
  -> reserve bounded amounts atomically
  -> send one provider attempt
  -> settle verified usage or retain the conservative debit
  -> report result and remaining application budget
```

For each affected meter, atomically require `consumed + reserved + requested <= effective_limit` at the existing D1 boundary. Do not implement admission as a read followed by an unrelated update or as process-local counters. Reserve the required meters together. Duplicate delivery of one attempt must not charge twice; a real retry has a new attempt identity and a fresh admission check. A rename or unit conversion resolves to the same normalized consumption identity, not a new free allowance.

A business operation touches several products: file reads use Worker requests, R2 reads and telemetry; writes additionally use R2 writes and D1 rows; hybrid search uses D1 retrieval, source reads and possibly AI Search; conversion may use Workers AI. Count adapter operations rather than treating one user action as one database row or one R2 request. D1 scans, indexes and the quota ledger itself consume rows; use returned metadata for settlement and conservative bounds where admission is possible. Incoming Worker requests and automatic logs can occur before admission.

Release only amounts known not to have been charged. A sent request that times out has unknown delivery/cost; retain its conservative debit until reconciled. Unsent leases can expire, while possibly delivered calls cannot be refunded merely by a timer. Keep in-flight reservations across resets and conservatively cover an uncertain provider charge window until reconciliation. Lower limits stop new work without cancelling the possible cost of already-queued indexing. Failed R2 requests can still be billable. [R2 pricing](https://developers.cloudflare.com/r2/pricing/).

## Honest enforcement and no double counting

| Resource or work | Required treatment |
| --- | --- |
| Controlled query/request attempts | Reserve each actual attempt; retain ambiguous charges conservatively. |
| Managed ingestion and model work | Final overlapping chunks and backend retries are opaque. Require a defensible bound under strict protection; otherwise pause publication with `unbounded_charge`. |
| D1 rows, Worker CPU and platform logs | Distinguish prior bounds, actual post-operation metadata and platform-enforced ceilings. Do not label every measurement as application-enforced. |
| Storage and retention | GB-month is storage over time; capacity and retention are separate. Stopping requests does not remove retained data or its future storage use. |
| Managed AI Search components | Do not also debit standalone R2, Vectorize, Browser Run or direct Workers AI inference for components already included in managed AI Search billing. Brains' own source R2/D1 work still counts. |
| Shared pools | New Gateway logs share the applicable Workers/Observability pool. Account-shared usage and other producers consume one allowance, not a copy per instance. |

Sources: [AI Search accounting](https://developers.cloudflare.com/ai-search/platform/limits-pricing/), [Items API](https://developers.cloudflare.com/ai-search/api/items/workers-binding/), [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/), [Gateway pricing](https://developers.cloudflare.com/ai-gateway/reference/pricing/).

The guarantee is bounded application admission for controlled measurable work. Other account applications, provider background work, incoming invocations and retained storage prevent an app-only account-wide zero-billing guarantee. A future self-managed embedding adapter could replace managed ingestion when dimension control or per-call metering is required; this release mounts managed AI Search only. Never delete authoritative history automatically to meet a watermark. Expose repairable pending publication and explicit source-preserving cleanup choices.

## Capabilities and quota status

Capabilities report installed features separately from temporary admission. Model dimensions, dimension controls, model modalities and accepted adapter inputs have distinct fields. This response excerpt illustrates an exhausted semantic-query budget:

```json
{
  "features": { "semantic_search": true },
  "semantic": {
    "model_id": "@cf/qwen/qwen3-embedding-0.6b",
    "dimensions": 1024,
    "vector_index_version": 1,
    "dimension_control": { "mode": "fixed", "supported": [1024] },
    "model_modalities": ["text"],
    "accepted_inputs": ["text"]
  },
  "admission": {
    "semantic_search": {
      "state": "blocked",
      "reason": "quota_exhausted",
      "rule": "search-semantic",
      "limit": 800,
      "consumed": 800,
      "reserved": 0,
      "remaining": 0
    }
  }
}
```

Use the same quota status shape for file writes, imports and other operations: a D1 write budget can block a file write without falsely declaring the feature unimplemented. Detailed authenticated usage reports additionally include resource/scope/window, measured evidence, unresolved consumption and reset time when known. Quota rejection uses a machine-readable 429; unconfigured, unbounded and unknown usage have separate reasons. Do not invent reset timestamps for capacity rules or unknown provider accounting.

Cache capability identity separately from current admission. Recheck the atomic budget on execution; cached readiness cannot authorize an over-budget request. Hybrid search may return keyword results with semantic unavailability reported, and saved files remain saved when publication is paused.

## Implementation and verification

Configuration, unit normalization, admission, settlement and provider metering live in separate small modules. Public operations and capability changes go through the shared API/SDK generator. Focused domain/application regressions and `tests/cloudflare-resources.py` protect alias/unit identity, concurrent admission, unresolved reservations, conditional conversion and partial imports.

Verify JSON rule shape, recognized units/periods, scope sharing, whole-account versus per-resource capacity, fraction calculation including fractional storage, 80% defaults and explicit overrides. Verify inactive products and eligible log generations, future pricing replacement without double counting, and that rule renames/config reloads do not reset usage. Implementation acceptance also needs concurrent/duplicate attempts, retries, ambiguous delivery, billing rollover, stale capabilities, fixed/adjustable dimensions, both model modalities and actual adapter inputs, and ranked retrieval without the provider's default threshold. A document check is not runtime acceptance.


## Implemented enforcement

| Meter or constraint | Current behavior |
| --- | --- |
| Dynamic Worker requests | Debit before routing; exhaustion returns structured 429, rule and reset time. Incoming requests outside this code still consume provider usage. |
| Source R2 Class A/B | Conservatively debit dispatched storage attempts, including conditional writes; failed attempts are not automatically refunded. Static assets are separate. |
| D1 rows | Use actual result metadata for application SQL and a conservative bookkeeping allowance for ledger work. Observe completed scans/writes and reject later admissions; this cannot pre-bound every SQLite scan. |
| D1 database size | Record `size_after` as a byte gauge and apply the configured per-database watermark before later writes. Account-wide storage remains observational. |
| AI Search semantic queries | Atomically reserve one attempt, consume on success and retain uncertain delivery rather than refund it. |
| AI Search managed ingestion | Source-byte count is explicitly estimated, not billable tokens. Default strict policy pauses it; `when_charge_unknown: "observe"` permits this opaque path. |
| Image conversion / external model billing | Strict mode blocks opaque costs. Explicit observation opts in without claiming a billable-neuron or Google-project hard bound. |
| Storage-time, logs/traces, Access/MCP seats and unmounted products | Retain provider facts and user policy in the catalog. Runtime does not invent account-wide usage or enforce these through unrelated request counters. Configure provider-side controls and telemetry separately. |
| CPU, memory, connection/query ceilings and retained-record limits | Platform constraints. The Worker also uses bounded semantic validation and smaller advertised reindex pages; lowering an informational catalog rule alone does not reconfigure the platform. |

Use `GET /api/v1/resources`, `bsx resources` or administrator settings to inspect the full catalog and measured usage. Ordinary capabilities separate mounted features from request/semantic-query/semantic-ingestion admission. Unknown ingestion is not a completed index; pending/blocked projections keep semantic coverage incomplete.

A stable meter identity excludes its window, alias, display unit and policy; each usage bucket has its own window key. Unresolved reservations remain visible and constrain the next window. A success received after its reserved window expires stays uncertain until reconciled, rather than being silently charged to the wrong period or released. Terminal settlement is idempotent. Operators can inspect `resource_attempts` and its original quantities in D1; release only when evidence proves an attempt was not charged. Do not clear usage tables to change a limit or retry a request.

These counters describe the configured deployment ledger. Other applications, other Brains ledgers, existing retained data and provider background work can use the same account allowance. Configure smaller allocations when sharing an account. The application does not promise account-wide zero billing, exact provider invoices, automatic current-price discovery or universal hard enforcement. Review the snapshot and Cloudflare's current pricing before enabling a product or raising an allowance.
