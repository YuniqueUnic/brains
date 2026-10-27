# Cloudflare feature verification

This record covers the cloud ingestion, retrieval and resource-policy implementation verified on October 27, 2026. Cloud tests used dedicated temporary Workers, R2, D1, AI Search and AI Gateway resources with synthetic documents. Existing production storage and credentials were not modified.

| Boundary | Verification and observed result |
| --- | --- |
| Rust workspace | Format, all-feature check, Clippy with warnings denied, and all-feature tests passed: 113 tests across 40 suites, with no failures or ignored tests. |
| Resource rules | Domain tests cover normalized units, stable meter identity, explicit overrides and billing windows. Production SQL checks admit exactly five of twenty concurrent requests against a budget of five, keep multi-meter reservations atomic, retain uncertain delivery across rollover, and settle once. |
| Conditional conversion | Application tests and a real Workers AI HTML conversion preserve the original, avoid unchanged revisions, and reject a concurrent destination edit. Partial binary imports preserve the saved source and report the failed companion separately. |
| Public URL import | Real HTTPS HTML and DOCX imports passed robots checks and conversion. DOCX source bytes match the original, the companion cites its stored source/revision, and a retry is unchanged. Invalid schemes return 400 and private targets return 403. |
| Semantic retrieval | Real Items publication and asynchronous processing produced vector hits through managed AI Search. Current-revision, permission and metadata checks were separately exercised with controlled application providers. |
| Source lifecycle | Real R2 purge/recreate passed. Four simultaneous purge/restore attempts each had one successful conditional write and one 412; reads matched the winning source state. |
| Cloud quotas | A full resource profile was deployed in multiple environment chunks. Actual D1/R2 accounting and estimated ingestion labels were returned. A deliberately small isolated request budget returned structured 429 responses, including correct HEAD behavior. |
| Thin client | A CLI built without default features called remote conversion, URL import, resource usage and capability endpoints successfully. |
| MCP | The official MCP client SDK negotiated the real deployed endpoint and called all three read-only tools. Missing credentials were rejected, mutation tools were absent, and the upstream reader was denied administrative APIs. Note citations matched the returned revision. |
| Web | Web build, lint, Worker binding checks, MCP checks and bundle passed. Desktop and mobile browser flows covered authentication, resource usage, conversion, opening results and URL validation. Screenshots showed no content overflow. |
| Generated contract | Regenerating API/SDK artifacts with the pinned compiler produced no additional drift. |

## Reproduce the checks

Run `mbx fmt --all -- --check`, `mbx check --workspace --all-features`, `mbx clippy --workspace --all-targets --all-features -- -D warnings`, `mbx test --workspace --all-features`, `python3 tests/cloudflare-resources.py`, `pnpm lint`, `pnpm --dir apps/web build`, `pnpm worker:check` and `pnpm mcp:check`. The CI workflow additionally checks API generation and platform builds.

For cloud verification, follow [the deployment guide](cloudflare.md) using independent resource names and a private environment file. After deployment, `pnpm cloudflare:verify` checks the core APIs and `pnpm cloudflare:mcp-verify` uses the official MCP SDK. Conversion, retrieval and lifecycle tests need synthetic source documents; credentials and document bodies must remain out of logs.

## Verification boundaries

Actual owner-only Access login and portal policy require a separately configured identity provider and were not tested. Direct authenticated MCP success does not verify Access SSO. Optional Google BYOK, native multimodal embeddings, self-managed Vectorize, Browser Run and answer generation were not exercised or presented as mounted providers.

The live semantic test explicitly allowed observation of opaque managed ingestion usage. Strict configuration pauses publication when no defensible charge bound is available. Billing-cycle dates in the isolated test were synthetic; production requires actual provider billing dates and a current pricing review. Accounting for controlled work does not cover every other application or provider background charge in an account.

Local validation does not substitute for hosted CI. See the pull request checks for Linux, musl and hosted Worker build results. Existing production deployment is a separate rollout decision.
