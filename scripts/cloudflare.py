#!/usr/bin/env python3
"""Configure, check, deploy and verify the existing Brains Cloudflare Worker."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import tomllib
import urllib.error
import urllib.parse
import urllib.request

from cloudflare_config import compose_services
from cloudflare_mcp import initialize_mcp, deploy_mcp
from cloudflare_setup import provision
from cloudflare_actions import configure as configure_actions
from cloudflare_credentials import sync_admin_token


ROOT = Path(__file__).resolve().parent.parent
WORKER = ROOT / "apps/worker"
APP_SECRETS = ("TOKEN_REGISTRY_JSON", "SESSION_SIGNING_KEY", "CURSOR_SIGNING_KEY")
SETTINGS = (
    "CLOUDFLARE_API_TOKEN",
    "CLOUDFLARE_ACCOUNT_ID",
    "BRAINS_WORKER_NAME",
    "BRAINS_R2_BUCKET",
    "BRAINS_D1_DATABASE",
    "BRAINS_D1_DATABASE_ID",
    "BRAINS_CUSTOM_DOMAIN",
    "BRAINS_CUSTOM_DOMAIN_ZONE_ID",
    "VAULT_ID",
    "MAX_TEXT_BYTES",
    "MAX_ATTACHMENT_BYTES",
    "ALLOWED_ORIGINS",
    "SEMANTIC_SEARCH",
    "BRAINS_APPLY_MIGRATIONS",
    "BRAINS_BASE_URL",
    "BRAINS_CLOUD_CONFIG",
    "BRAINS_CLOUD_CONFIG_FILE",
    "BRAINS_RESOURCE_PROFILE",
    "BRAINS_RESOURCE_PROFILE_FILE",
    "BRAINS_RESOURCE_ACCOUNT",
    "BRAINS_AI_SEARCH_NAMESPACE",
    "BRAINS_RECONCILE_CRON",
    "BRAINS_R2_LIST_PAGE_MAX",
    "BRAINS_WORKER_CPU_MS",
    "BRAINS_WORKER_SUBREQUESTS",
    "BRAINS_WORKER_LOG_SAMPLE_RATE",
    "BRAINS_AI_GATEWAY_ID",
    "BRAINS_MCP_API_TOKEN",
    "BRAINS_MCP_ACCESS_TOKEN",
    "BRAINS_MCP_WORKER_NAME",
    "BRAINS_MCP_HOSTNAME",
    "BRAINS_MCP_READ_PREFIX",
    "BRAINS_ADMIN_TOKEN",
    *APP_SECRETS,
)


def load_environment(path):
    env = os.environ.copy()
    if path.exists():
        # Node implements dotenv parsing, including quoted JSON. Inherited
        # environment variables take precedence, as they do in GitHub Actions.
        code = (
            "console.log(JSON.stringify(Object.fromEntries("
            f"{json.dumps(SETTINGS)}.filter(k => process.env[k] !== undefined)"
            ".map(k => [k, process.env[k]]))))"
        )
        result = subprocess.run(
            ["node", f"--env-file={path}", "-e", code],
            cwd=ROOT,
            env=env,
            capture_output=True,
            text=True,
            check=True,
        )
        env.update(json.loads(result.stdout))
    env.update(
        WRANGLER_SEND_METRICS="false", CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV="false"
    )
    return env


def require(env, names):
    missing = [name for name in names if not env.get(name)]
    if missing:
        raise ValueError("Configure: " + ", ".join(missing))


def initialize(path, env):
    if os.environ.get("GITHUB_ACTIONS") == "true":
        raise ValueError(
            "Initialize credentials locally; configure Actions secrets explicitly"
        )
    if not path.exists():
        path.write_text((ROOT / ".env.example").read_text())
    path.chmod(0o600)
    configured = [bool(env.get(key)) for key in APP_SECRETS]
    if any(configured) and not all(configured):
        raise ValueError(
            "Partial instance secrets: provide all three; existing keys are never replaced"
        )
    if not any(configured):
        token = env.get("BRAINS_ADMIN_TOKEN") or secrets.token_urlsafe(32)
        registry = json.dumps(
            {
                "tokens": [
                    {
                        "id": "owner",
                        "name": "owner",
                        "enabled": True,
                        "token_sha256": hashlib.sha256(token.encode()).hexdigest(),
                        "scopes": ["read:/", "write:/", "admin:/"],
                    }
                ]
            },
            separators=(",", ":"),
        )
        values = {
            "TOKEN_REGISTRY_JSON": registry,
            "SESSION_SIGNING_KEY": secrets.token_hex(32),
            "CURSOR_SIGNING_KEY": secrets.token_hex(32),
        }
        if not env.get("BRAINS_ADMIN_TOKEN"):
            values["BRAINS_ADMIN_TOKEN"] = token
        # Fill generated credentials in place, preserving unrelated settings.
        pending = values.copy()
        lines = []
        for line in path.read_text().splitlines():
            key = line.partition("=")[0].strip()
            if key in values:
                if key in pending:
                    lines.append(f"{key}='{pending.pop(key)}'")
            else:
                lines.append(line)
        lines.extend(f"{key}='{value}'" for key, value in pending.items())
        path.write_text("\n".join(lines) + "\n")
        print("Instance credentials saved privately; no values printed.")
    print(f"Configuration: {path}")
    print(
        "Fill Cloudflare credentials and the real R2/D1 target. See docs/cloudflare.md."
    )
    print("This command creates no cloud resources and does not deploy.")


def deployment_config(env):
    config = tomllib.loads((WORKER / "wrangler.toml").read_text())
    for key in (
        "BRAINS_WORKER_NAME",
        "BRAINS_R2_BUCKET",
        "BRAINS_D1_DATABASE",
        "BRAINS_D1_DATABASE_ID",
    ):
        require(env, [key])
    if env["BRAINS_D1_DATABASE_ID"].strip() == "local":
        raise ValueError(
            "BRAINS_D1_DATABASE_ID must be a real remote database ID, not local"
        )
    config["name"] = env["BRAINS_WORKER_NAME"]
    if env.get("BRAINS_CUSTOM_DOMAIN"):
        require(env, ("BRAINS_CUSTOM_DOMAIN_ZONE_ID",))
        config.update(workers_dev=False, preview_urls=False)
        config["routes"] = [
            {
                "pattern": env["BRAINS_CUSTOM_DOMAIN"],
                "custom_domain": True,
                "zone_id": env["BRAINS_CUSTOM_DOMAIN_ZONE_ID"],
            }
        ]
    config["main"] = str((WORKER / config["main"]).resolve())
    config["assets"]["directory"] = str(
        (WORKER / config["assets"]["directory"]).resolve()
    )
    config["build"]["cwd"] = str(WORKER)
    config["r2_buckets"][0]["bucket_name"] = env["BRAINS_R2_BUCKET"]
    database = config["d1_databases"][0]
    database.update(
        database_name=env["BRAINS_D1_DATABASE"],
        database_id=env["BRAINS_D1_DATABASE_ID"],
    )
    database["migrations_dir"] = str((WORKER / database["migrations_dir"]).resolve())
    for name in (
        "VAULT_ID",
        "MAX_TEXT_BYTES",
        "MAX_ATTACHMENT_BYTES",
        "ALLOWED_ORIGINS",
        "SEMANTIC_SEARCH",
    ):
        if env.get(name) or (name == "ALLOWED_ORIGINS" and name in env):
            config["vars"][name] = env[name]
    for name in ("MAX_TEXT_BYTES", "MAX_ATTACHMENT_BYTES"):
        if int(config["vars"][name]) < 0:
            raise ValueError(f"{name} must be non-negative")
    compose_services(config, env)
    return config


def instance_secrets(env):
    supplied = {key: env[key] for key in APP_SECRETS if env.get(key)}
    if supplied:
        require(env, APP_SECRETS)
        registry = json.loads(supplied["TOKEN_REGISTRY_JSON"])
        tokens = registry.get("tokens") if isinstance(registry, dict) else None
        if not isinstance(tokens, list) or not tokens:
            raise ValueError(
                "TOKEN_REGISTRY_JSON must contain a non-empty tokens array"
            )
        for token in tokens:
            valid = isinstance(token, dict) and all(
                isinstance(token.get(key), str)
                for key in ("id", "name", "token_sha256")
            )
            if not valid or not isinstance(token.get("enabled"), bool):
                raise ValueError(
                    "Each token needs id, name, token_sha256, enabled and scopes"
                )
            if token.get("plaintext") is not None:
                raise ValueError(
                    "TOKEN_REGISTRY_JSON must contain hashes, not plaintext tokens"
                )
            if not isinstance(token.get("scopes"), list) or not all(
                isinstance(s, str) for s in token["scopes"]
            ):
                raise ValueError("Token scopes must be a list of strings")
            if any(
                ":" not in scope
                or scope.split(":", 1)[0] not in ("read", "write", "admin")
                for scope in token["scopes"]
            ):
                raise ValueError("Each token scope must use read:, write: or admin:")
        require(env, ("BRAINS_ADMIN_TOKEN",))
        digest = hashlib.sha256(env["BRAINS_ADMIN_TOKEN"].encode()).hexdigest()
        if not any(
            t["enabled"] and t["token_sha256"] == digest and "admin:/" in t["scopes"]
            for t in tokens
        ):
            raise ValueError(
                "BRAINS_ADMIN_TOKEN must match an enabled registry token with admin:/"
            )
        for key in APP_SECRETS[1:]:
            if len(supplied[key]) != 64 or len(bytes.fromhex(supplied[key])) != 32:
                raise ValueError(f"{key} must be 64 hexadecimal characters")
        if supplied["SESSION_SIGNING_KEY"] == supplied["CURSOR_SIGNING_KEY"]:
            raise ValueError("Session and cursor signing keys must be different")
    return supplied


def run(command, env):
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def wrangler(env, config, *args):
    run(
        [
            "pnpm",
            "--dir",
            str(WORKER),
            "exec",
            "wrangler",
            *args,
            "--config",
            str(config),
        ],
        env,
    )


def base_url(env, allow_loopback=False):
    require(env, ("BRAINS_BASE_URL",))
    base = env["BRAINS_BASE_URL"].rstrip("/")
    url = urllib.parse.urlsplit(base)
    local = allow_loopback and url.hostname in ("localhost", "127.0.0.1", "::1")
    if not url.hostname or not (
        url.scheme == "https" or url.scheme == "http" and local
    ):
        raise ValueError("BRAINS_BASE_URL must be an HTTPS origin")
    if url.username or url.password or url.query or url.fragment or url.path:
        raise ValueError(
            "BRAINS_BASE_URL must not contain credentials, a path, query or fragment"
        )
    return base


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def verify(env):
    require(env, ("BRAINS_BASE_URL", "BRAINS_ADMIN_TOKEN"))
    base = base_url(env, allow_loopback=True)
    opener = urllib.request.build_opener(NoRedirect())

    def request(path, authenticated=False, body=None):
        headers = {"User-Agent": "brains-cloudflare-check"}
        if authenticated:
            headers["Authorization"] = "Bearer " + env["BRAINS_ADMIN_TOKEN"]
        if body is not None:
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(
            base + path,
            headers=headers,
            data=json.dumps(body).encode() if body is not None else None,
        )
        try:
            with opener.open(req, timeout=30) as response:
                return response.read(), response.headers
        except urllib.error.HTTPError as error:
            raise ValueError(
                f"Verification failed at {path}: HTTP {error.code}"
            ) from error

    request("/healthz")
    page, headers = request("/")
    if "text/html" not in headers.get("Content-Type", "") or not page:
        raise ValueError("The Worker did not serve the web application")
    caps = json.loads(request("/api/v1/capabilities", True)[0])
    if caps["runtime"] != "cloudflare-rust" or not caps["features"]["fts_search"]:
        raise ValueError("Expected the full core cloudflare-rust Worker with D1 FTS")
    if caps["vault"] != env.get("VAULT_ID", "default"):
        raise ValueError("The live vault differs from VAULT_ID")
    # Health alone never proves that D1 migrations or R2 permissions work.
    request("/api/v1/files/query", True, {"source": "index", "limit": 1})
    request("/api/v1/files/query", True, {"source": "store", "limit": 1})
    print(
        f"Verified web, authentication, core capabilities, D1 and R2; build {caps['build_version']}"
    )


def deploy(args, env):
    config = deployment_config(env)
    supplied = instance_secrets(env)
    if args.command == "deploy":
        require(env, ("CLOUDFLARE_ACCOUNT_ID", "BRAINS_BASE_URL", "BRAINS_ADMIN_TOKEN"))
        if env.get("GITHUB_ACTIONS") == "true":
            require(env, ("CLOUDFLARE_API_TOKEN",))
        base_url(env)
        if not supplied:
            verify(env)  # Updates without secrets require an already usable instance.
    migrate = env.get("BRAINS_APPLY_MIGRATIONS", "true").lower()
    if migrate not in ("true", "false"):
        raise ValueError("BRAINS_APPLY_MIGRATIONS must be true or false")
    if not args.skip_build:
        run(["pnpm", "--dir", "apps/web", "build"], env)
    else:
        config.pop("build", None)
    # Credentials stay out of the repository, logs and command arguments.
    with tempfile.TemporaryDirectory(prefix="brains-cloudflare-") as directory:
        root = Path(directory)
        path = root / "wrangler.json"
        path.write_text(json.dumps(config))
        secret_args = []
        if supplied:
            secret_path = root / "secrets.json"
            secret_path.write_text(json.dumps(supplied))
            secret_path.chmod(0o600)
            secret_args = ["--secrets-file", str(secret_path)]
        build_env = env.copy()
        flags = build_env.get("RUSTFLAGS", "")
        build_env["RUSTFLAGS"] = (flags + ' --cfg getrandom_backend="wasm_js"').strip()
        wrangler(
            build_env,
            path,
            "deploy",
            "--dry-run",
            "--outdir",
            str(root / "bundle"),
            *secret_args,
        )
        run(
            [
                "python3",
                "scripts/run-workerd.py",
                "--",
                "bash",
                "tests/worker-smoke.sh",
            ],
            env,
        )
        if args.command == "check":
            print("Local deployment checks passed; no cloud resources changed.")
            return
        # Reuse the exact Wasm/Web inputs just checked instead of rebuilding Rust.
        config.pop("build", None)
        path.write_text(json.dumps(config))
        if migrate == "true":
            wrangler(env, path, "d1", "migrations", "apply", "BRAINS_INDEX", "--remote")
        wrangler(env, path, "deploy", *secret_args)
        verify(env)


def binding_types(env):
    config = tomllib.loads((WORKER / "wrangler.toml").read_text())
    config.pop("build", None)
    config["main"] = str((WORKER / config["main"]).resolve())
    config["assets"]["directory"] = str(
        (WORKER / config["assets"]["directory"]).resolve()
    )
    with tempfile.TemporaryDirectory(prefix="brains-types-") as directory:
        path = Path(directory) / "wrangler.json"
        path.write_text(json.dumps(config))
        wrangler(
            env,
            path,
            "types",
            str(WORKER / "worker-configuration.d.ts"),
            "--env-interface",
            "WorkerEnv",
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=(
            "init",
            "check",
            "deploy",
            "verify",
            "wrangler",
            "provision",
            "mcp-init",
            "mcp-check",
            "mcp-deploy",
            "mcp-verify",
            "types",
            "github-configure",
            "sync-admin-token",
        ),
    )
    parser.add_argument("--env-file", type=Path, default=ROOT / ".env")
    parser.add_argument("--github-repository")
    parser.add_argument("--github-environment", default="production")
    parser.add_argument(
        "--skip-build", action="store_true", help="use already built Worker/Web assets"
    )
    args, extra = parser.parse_known_args()
    if extra and args.command != "wrangler":
        parser.error("unrecognized arguments: " + " ".join(extra))
    try:
        env = load_environment(args.env_file.resolve())
        if args.command == "init":
            initialize(args.env_file.resolve(), env)
        elif args.command == "sync-admin-token":
            sync_admin_token(args.env_file.resolve(), env)
            instance_secrets(env)
        elif args.command == "github-configure":
            instance_secrets(env)
            if not args.github_repository:
                raise ValueError("Set --github-repository to the deployment repository")
            configure_actions(env, args.github_repository, args.github_environment)
        elif args.command == "types":
            binding_types(env)
        elif args.command == "provision":
            provision(env)
        elif args.command == "mcp-init":
            initialize_mcp(args.env_file.resolve(), env)
        elif args.command in ("mcp-check", "mcp-deploy", "types"):
            deploy_mcp(env, dry_run=args.command == "mcp-check")
        elif args.command == "mcp-verify":
            run(["node", "apps/mcp/scripts/verify.mjs"], env)
        elif args.command == "verify":
            verify(env)
        elif args.command == "wrangler":
            wrangler(env, WORKER / "wrangler.toml", *extra)
        else:
            deploy(args, env)
    except (
        ValueError,
        OSError,
        subprocess.CalledProcessError,
        urllib.error.URLError,
    ) as error:
        # A subprocess error must not echo a secret-bearing env or response body.
        message = (
            f"command failed (exit {error.returncode})"
            if isinstance(error, subprocess.CalledProcessError)
            else str(error)
        )
        parser.exit(1, f"Cloudflare: {message}\n")


if __name__ == "__main__":
    main()
