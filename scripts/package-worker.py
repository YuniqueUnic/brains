#!/usr/bin/env python3
"""Package prebuilt Worker assets for deployment without a source checkout (Python 3.11+)."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory for the unpacked bundle")
    parser.add_argument("--version", required=True, help="validated Cargo release version")
    parser.add_argument("--sha", required=True, help="commit used to build the bundle")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    worker = repo / "apps/worker"
    config = tomllib.loads((worker / "wrangler.toml").read_text())
    wrangler = json.loads((worker / "node_modules/wrangler/package.json").read_text())["version"]
    build = worker / "build"
    assets = (worker / config["assets"]["directory"]).resolve()
    migrations = repo / "migrations/d1"
    for required in (worker / config["main"], build / "index.js", build / "index_bg.wasm", assets / "index.html"):
        if not required.is_file():
            parser.error(f"missing build input: {required}")
    if not any(migrations.glob("*.sql")):
        parser.error("D1 migrations are missing")

    # Keep runtime bindings, routing and limits; replace only checkout paths.
    config.pop("build", None)
    config["assets"]["directory"] = "web/dist"
    for database in config["d1_databases"]:
        database["migrations_dir"] = "migrations/d1"
    dirty = bool(subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=repo, text=True,
    ).strip())
    metadata = {
        "tag": f"v{args.version}", "version": args.version,
        "git_sha": args.sha, "git_dirty": dirty, "wrangler_version": wrangler,
    }

    args.output.mkdir(parents=True, exist_ok=False)
    shutil.copytree(build, args.output / "build")
    shutil.copytree(assets, args.output / "web/dist")
    shutil.copytree(migrations, args.output / "migrations/d1")
    (args.output / "wrangler.json").write_text(json.dumps(config, indent=2) + "\n")
    (args.output / "release.json").write_text(json.dumps(metadata, indent=2) + "\n")
    (args.output / "README.md").write_text(f"""# Brains Worker {args.version}

This bundle includes compiled Worker/Wasm, the web app, D1 migrations and a deployment configuration. No Rust toolchain, Cargo source or separate web bundle is needed. Build identity and the tested Wrangler version are in `release.json`. Archive SHA-256 is provided alongside the download.

Use Node.js 22+ and run all commands from this unpacked directory. The commands below use the exact Wrangler version installed from the source lockfile.

## Configure an isolated target

Authenticate with Cloudflare using your own account credentials. Create a dedicated R2 bucket and D1 database if the target does not exist. In `wrangler.json`, set `name`, `r2_buckets[0].bucket_name`, `d1_databases[0].database_name`, the real `database_id` (replace `local`), and `vars.VAULT_ID`. Keep the `BRAINS_BUCKET` and `BRAINS_INDEX` binding names. Changing VAULT_ID alone does not isolate an existing D1 database.

Prepare a private JSON file outside this directory (mode 0600) containing three string values:

- `TOKEN_REGISTRY_JSON`: JSON-encoded `{{"tokens":[{{"id":"owner","name":"owner","enabled":true,"token_sha256":"<SHA-256 of your bearer token>","scopes":["read:/","write:/","admin:/"]}}]}}`. Retain the original bearer token securely for CLI/browser login.
- `SESSION_SIGNING_KEY`: 32 random bytes encoded as 64 hexadecimal characters.
- `CURSOR_SIGNING_KEY`: a different 32 random bytes encoded as 64 hexadecimal characters.

Do not commit or place this private file under `web/dist`. No credentials are included in this bundle. The required secret names remain enforced by Wrangler.

## Verify locally, then deploy

The dry run bundles local files without deploying. The next two commands modify your cloud database and Worker; run them only for the target you intend to deploy.

```bash
npx --yes wrangler@{wrangler} deploy --config wrangler.json --dry-run --outdir dry-run
npx --yes wrangler@{wrangler} d1 migrations apply BRAINS_INDEX --config wrangler.json --remote
npx --yes wrangler@{wrangler} deploy --config wrangler.json --secrets-file /absolute/private/brains-secrets.json
```

For subsequent deployments, keep the same instance secrets unless deliberately rotating them. The configuration has no custom build command: deployment uses this bundle's compiled files.

## Migrate content and switch the CLI

Use a new empty destination for full-history restoration; do not seed or sync it first. Pause source writes and retain the source data and archive. Install the matching `bsx` native package, then connect both named profiles using `profile add` and `login --stdin-token`.

```bash
unset BRAINS_URL BRAINS_TOKEN
bsx --profile local export ./full-archive --format archive
bsx --profile cloud import ./full-archive --format archive --dry-run
bsx --profile cloud import ./full-archive --format archive
bsx --profile local sync --remote-profile cloud verify
bsx profile use cloud --verify-with local
```

Require `history_verified` and `index_verified` from the import before switching; sync verification alone checks current live content, not history. A failed import may leave an exact historical prefix: retain the same archive, keep the target out of daily use and retry. A failed verification must not be treated as a completed migration. This package does not prove cloud permissions, quotas or production latency.
""")
    print(f"Worker bundle ready: {args.output}")


if __name__ == "__main__":
    main()
