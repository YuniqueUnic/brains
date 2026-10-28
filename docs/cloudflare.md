# Deploy the existing Brains Worker

Cloudflare runs the Brains API and web app, with R2 for versioned content and D1 for derived indexes and usage accounting. Optional Worker providers add document conversion, public URL import and managed AI Search. A remote `bsx`, including a thin build without a native converter, can use those APIs.

Core-only deployments retain file storage, conditional writes, history, trash/restore, lexical search, archive and sync. Cloud services are opt-in: a switch alone does not mount a provider or create an index. AI Search uses managed vectors; standalone Vectorize, Browser Run and answer generation are separate extensions. See [capabilities](capabilities.md) and [cloud architecture](cloudflare-ai-plan.md).

Read [cost amplification and controls](cloudflare-cost-safety.md) before enabling background services or increasing invocation/scan limits. The application ledger is not an account-wide spending cap.

## Requirements

- Node.js 22+, Python 3.11+, pnpm and the repository Rust/Wasm toolchain.
- A Cloudflare account with R2 and D1 enabled. Local deployment can use Wrangler OAuth login or an API token; GitHub Actions requires an API token allowed to deploy Workers and manage the target R2/D1 resources.
- A dedicated R2 bucket and D1 database for this instance. D1 indexes paths without a vault column: changing `VAULT_ID` does not isolate a shared database.
- The expected Worker URL, normally `https://<worker>.<account-subdomain>.workers.dev` or your configured custom domain.

Local deployment and GitHub Actions use the same `scripts/cloudflare.py` entry point. It reads the existing Wrangler TOML and generates a temporary deployment config; you do not edit a database ID into tracked files. Node's built-in dotenv loader reads the root `.env`, with inherited environment variables taking precedence.

The Worker configuration samples invocation logs and traces at 0.1 by default, with independent settings. Request query strings are redacted from these logs. Inspect the Worker logs in the Cloudflare Dashboard when investigating a failure that does not reproduce locally.

## First deployment

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm cloudflare:init
```

`init` creates the ignored root `.env` from [.env.example](../.env.example), generates an admin token, hashed token registry and two signing keys, and fills the credential placeholders in that file with owner-only permissions. The template remains empty; the initialized `.env` contains the real values. It never prints credentials, rotates existing complete secrets, creates cloud resources or deploys. Retain the same credentials for subsequent deployments. If you supply a custom registry, keep an enabled admin token for verification.

Fill `CLOUDFLARE_ACCOUNT_ID` in `.env`. For local deployment, authenticate Wrangler in your browser, or set `CLOUDFLARE_API_TOKEN` instead. Cloudflare MCP authentication is separate from Wrangler authentication:

```sh
pnpm cloudflare:wrangler login
pnpm cloudflare:wrangler whoami
```

Create the resources once with Wrangler, using your chosen names. The wrapper loads root dotenv credentials before invoking Wrangler; these commands modify the cloud account:

```sh
pnpm cloudflare:wrangler r2 bucket create brains-data
pnpm cloudflare:wrangler d1 create brains-index
```

Fill the returned D1 ID into `BRAINS_D1_DATABASE_ID`, check the resource names, and set `BRAINS_BASE_URL` to the intended instance URL. Resource creation uses the official Wrangler commands rather than a second infrastructure implementation. Existing dedicated resources can be reused by configuring their names and real ID; `local` is rejected.

For a custom domain, set `BRAINS_CUSTOM_DOMAIN` to the hostname and `BRAINS_CUSTOM_DOMAIN_ZONE_ID` to its Cloudflare zone ID, then set `BRAINS_BASE_URL` to that hostname's HTTPS origin. The deployment configuration declares this Worker custom domain and disables `workers.dev` and preview URLs, so later deployments retain the selected public entry point. The zone must belong to the selected account, and the hostname must be available for this Worker. Cloudflare provisions the DNS record and certificate. See [Workers custom domains](https://developers.cloudflare.com/workers/configuration/routing/custom-domains/).

Install the build tool, then check and deploy:

```sh
mbx install worker-build --locked --version 0.8.7
pnpm cloudflare:check
pnpm cloudflare:deploy
```

`check` builds the web app and Worker, runs Wrangler's local dry run and the existing isolated Worker smoke suite. It makes no cloud changes. `deploy` performs those same checks, applies remote D1 migrations unless disabled, deploys the checked inputs without rebuilding Rust, then verifies the configured live URL. You can run `deploy` directly; a separate `check` is useful for review before any remote write.

Three app secrets are sent together with the code through `deploy --secrets-file`. Cloudflare management credentials and the plaintext admin token are never included in that file. If all three app secrets are omitted in an update, Wrangler retains the previous values; partial sets are rejected. Do not use the whole root `.env` as a Worker secrets file. [Cloudflare secrets](https://developers.cloudflare.com/workers/configuration/secrets/)

## Configuration

| Input | Purpose |
| --- | --- |
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare account that owns this instance and its resources. |
| `CLOUDFLARE_API_TOKEN` | Deployment credential for GitHub Actions or token-based local deployment. Optional locally when Wrangler is logged in through OAuth; separate from application authentication. |
| `BRAINS_WORKER_NAME`, `BRAINS_R2_BUCKET` | Worker and existing dedicated R2 bucket names. |
| `BRAINS_D1_DATABASE`, `BRAINS_D1_DATABASE_ID` | Existing D1 database name and real ID. |
| `BRAINS_BASE_URL` | Expected instance URL, used for post-deployment verification. |
| `BRAINS_CUSTOM_DOMAIN`, `BRAINS_CUSTOM_DOMAIN_ZONE_ID` | Optional hostname and its zone ID for a Worker custom domain. Set both when using a custom domain. |
| `BRAINS_ADMIN_TOKEN` | Private client login token used for authenticated verification; never uploaded as plaintext. |
| `TOKEN_REGISTRY_JSON` | Worker registry containing token hashes, scopes and enabled flags. |
| `SESSION_SIGNING_KEY`, `CURSOR_SIGNING_KEY` | Different 32-byte keys, each encoded as 64 hex characters. |
| `VAULT_ID` | Single vault managed by this instance. |
| `MAX_TEXT_BYTES` | Text indexing threshold; larger content can still be stored up to the attachment limit. |
| `MAX_ATTACHMENT_BYTES` | Maximum stored content size. |
| `ALLOWED_ORIGINS` | Optional comma-separated origins for cross-origin cookie flows; empty means same-origin only. |
| `SEMANTIC_SEARCH` | `off` prevents mounting semantic retrieval; `on` uses the configured provider when bindings and resource policy are present. |
| `BRAINS_APPLY_MIGRATIONS` | `true` by default; `false` skips remote migration application. |

There is no `FULL_MODE` or generic Worker `BRAINS_DISABLE` switch. Native compile-time features and server advertisement switches are not Worker runtime controls. Every deployment derives ordinary variables from this configuration, so Dashboard-only changes may be overwritten. [Wrangler configuration](https://developers.cloudflare.com/workers/wrangler/configuration/)

## GitHub Actions

Create a GitHub Environment such as `production`, then configure these environment or repository secrets:

```text
CLOUDFLARE_ACCOUNT_ID
CLOUDFLARE_API_TOKEN
BRAINS_ADMIN_TOKEN
TOKEN_REGISTRY_JSON
SESSION_SIGNING_KEY
CURSOR_SIGNING_KEY
```

Use the values generated by local initialization; do not regenerate them on every run. Set the non-secret inputs from the table as Environment variables. Worker/bucket/database names, vault and size limits have the defaults shown in `.env.example`; D1 ID and base URL must be supplied.

Configure the selected GitHub Environment from the private local file:

```sh
pnpm cloudflare:sync-admin-token
pnpm cloudflare:github-configure --github-repository OWNER/REPOSITORY
```

The owner token must be nonempty printable ASCII without whitespace; no minimum length is imposed by the application. `sync-admin-token` updates only the enabled default owner's hash, preserving signing keys and other principals. Custom registries must identify their intended admin explicitly. Never edit a token in `.env` and assume that the deployed registry already contains its new hash.

`github-configure` uploads secrets over stdin and writes public settings as Environment variables. JSON file inputs are resolved into inline variables for Actions. Explicitly empty optional values remove stale variables; omitted values remain untouched. Initial setup creates a `production` Environment limited to `main`. Existing protection rules are retained. Run the command again after changing local credentials or deployment settings.

**Deploy (Cloudflare Workers)** runs automatically after a successful `CI` push run on the current `main` commit. It checks out that exact tested SHA, skips obsolete CI completions, and serializes deployments per environment. The manual workflow remains available for a selected Environment. Both entry points use `pnpm cloudflare:deploy`, frozen dependencies and the pinned Worker build tool. Separate environments need their own Worker, R2, D1 and secrets. [Cloudflare GitHub Actions guide](https://developers.cloudflare.com/workers/ci-cd/external-cicd/github-actions/)

## Verify and connect bsx

```sh
pnpm cloudflare:verify
bsx profile add cloud --url https://<worker>.<account-subdomain>.workers.dev
# Supply the admin token privately through stdin:
bsx --profile cloud login --stdin-token
bsx --profile cloud capabilities
bsx --profile cloud whoami
```

Verification checks the web page, health, authenticated runtime capabilities, the expected vault, a real D1 index query and an R2 store listing. Authentication is sent only to the configured origin and redirects are rejected; cloud deployment requires HTTPS. The standalone verifier also permits HTTP loopback for isolated local checks. Verification is read-only and does not claim a write/restore, exact deployed-SHA or production-load test. The isolated smoke suite separately exercises conditional writes, conflicts, search, trash, restore and history.

Deployment does not migrate a local library, change its default profile or synchronize it. Full-history migration is a separate explicit operation: retain a source archive and verify the empty destination and import before switching use. See the [runbook](runbook.md) and [portable workflow](portable-workflow.md).

For an intentional reuse of already-built Worker/Web inputs, the helper accepts `--skip-build`. An alternative dotenv path can be supplied with `--env-file /private/path/.env`; this also makes isolated configuration checks independent of the root `.env`.


## Enable cloud conversion and managed search

Copy [cloudflare-services.json](examples/cloudflare-services.json) and [the dated resource profile](examples/cloudflare-free-2026-10-27.json) into a private configuration directory. Keep the service instance/model aligned with the actual AI Search instance. Fill real R2 and AI Search billing-window bounds in `context.billing_cycles`; unset or expired required windows block protected operations. Review the latest Cloudflare pricing before using the snapshot. The default allowance policy is 80%, not an account-wide billing guarantee.

```dotenv
SEMANTIC_SEARCH=on
BRAINS_CLOUD_CONFIG_FILE=secrets/cloud-services.json
BRAINS_RESOURCE_PROFILE_FILE=secrets/cloud-resources.json
BRAINS_AI_SEARCH_NAMESPACE=default
BRAINS_AI_GATEWAY_ID=brains-notes
BRAINS_RECONCILE_CRON='*/5 * * * *'
```

The root `.env` and `secrets/` are ignored. The helper loads each JSON input, assembles matching bindings and reassembles the full resource catalog through sub-5-KB Worker vars. On Actions use `BRAINS_CLOUD_CONFIG` and `BRAINS_RESOURCE_PROFILE` JSON variables instead of local filenames. Inline and file variants are mutually exclusive.

```sh
# Requires an API token with AI Search and AI Gateway management permissions.
# Wrangler OAuth alone can still deploy pre-provisioned bindings.
pnpm cloudflare:provision
pnpm cloudflare:check
pnpm cloudflare:deploy
pnpm cloudflare:verify
bsx --profile cloud resources
```

Provisioning creates or verifies a dedicated Gateway and upload-only AI Search instance. It disables public search, Gateway caching, rate limits and payload collection by default. It uploads no notes. If choosing a non-default namespace, create that namespace first. The default BGE-M3 model at 1,024 dimensions needs no external key. A Google model requires separately configured Gateway BYOK, supported managed dimensions and explicit external-usage observation; it has a separate provider bill.

Managed ingestion has no documented per-attempt billable-token upper bound. The default `when_charge_unknown: "block"` pauses it before upload. To opt in, set `policy.when_charge_unknown: "observe"` on the full `search-ingestion` rule in your private profile. Its byte-derived counter remains labeled estimated. Apply the same deliberate policy review to image conversion; an unknown amount is not a zero-cost inference. See [enforcement details](cloudflare-quotas.md).

The example selects `semantic.publication: "manual"`: normal writes queue sources and remove obsolete projections without uploading new ones, and deployment installs no reconciliation cron. Use `python3 scripts/cloudflare.py refresh-search` for an explicit bounded refresh. `automatic` mode permits mutation/cron processing. Queries validate current source revisions and permissions before returning snippets, and bounded/incomplete coverage remains explicit. Follow [the managed search procedure](cloudflare-managed-search.md) for a controlled backfill, provider usage calibration and subsequent manual refreshes.

The web upload menu exposes conversion and URL import when providers are mounted and the session can write. The **Usage & allowances** tab in administrator settings shows resource rules, consumption, retained reservations and evidence. Provider allowances, application budgets and informational platform limits remain distinct. The UI supports English and Simplified Chinese; the browser demo labels its usage as simulated and saves policy only in that browser.

Policy edits are stored in D1 with a revision precondition. Saved settings override the deployment bootstrap profile and survive redeployment; conflicts return 412 instead of replacing another administrator's changes. Editing a budget does not clear usage. Accounted billing boundaries cannot be changed to recover an overlapping allowance; enter the next non-overlapping period when appropriate. Use actual provider UTC period bounds and review current pricing. The settings page does not automatically renew billing periods.

The settings flag `enforced` controls admission independently from measurement. With enforcement off, configured providers remain available and measurable usage continues to be recorded. A missing billing period leaves that period's usage unmeasured; it does not invent a calendar-month allowance. Authentication, capabilities, health and resource administration remain reachable when an application budget is exhausted, so an owner can log in and repair it. Other protected operations still honor admission. This recovery path does not bypass authentication, CSRF or the provider's own hard limits. Core-only deployments expose the default resource catalog with enforcement off until an owner configures and enables it.

For an optional MCP Worker and owner-only portal, follow [the MCP guide](cloudflare-mcp.md). Actions can deploy the MCP Worker after the core when `deploy_mcp` is selected and its dedicated secrets/hostname are configured.

R2 purge uses an ETag-conditional zero-byte HEAD vacancy marker so a concurrent restore cannot be erased by an unconditional delete. Listings and reads hide these markers; create-only writes can reuse the path. Current filesystem adapters also understand them in raw R2 backups. Restore raw backups with this version or newer; HTTP archives keep their normal logical file representation.
