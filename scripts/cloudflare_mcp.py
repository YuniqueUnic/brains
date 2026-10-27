"""Keep MCP credentials and deployment separate from the vault owner's token."""

import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def initialize_mcp(path, env):
    if os.environ.get("GITHUB_ACTIONS") == "true":
        raise ValueError(
            "Initialize MCP credentials locally and configure Actions secrets explicitly"
        )
    if not env.get("TOKEN_REGISTRY_JSON"):
        raise ValueError("Run cloudflare:init first")
    if env.get("BRAINS_MCP_API_TOKEN") or env.get("BRAINS_MCP_ACCESS_TOKEN"):
        if not all(
            env.get(k) for k in ("BRAINS_MCP_API_TOKEN", "BRAINS_MCP_ACCESS_TOKEN")
        ):
            raise ValueError("Both MCP credentials must be configured together")
        print("Existing MCP credentials retained.")
        return
    registry = json.loads(env["TOKEN_REGISTRY_JSON"])
    if any(t.get("id") == "mcp-reader" for t in registry["tokens"]):
        raise ValueError(
            "An mcp-reader registry entry already exists; recover or explicitly rotate its credential"
        )
    token = secrets.token_urlsafe(32)
    prefix = env.get("BRAINS_MCP_READ_PREFIX", "/")
    if not prefix.startswith("/") or ":" in prefix or ".." in prefix.split("/"):
        raise ValueError("BRAINS_MCP_READ_PREFIX must be an absolute vault prefix")
    registry["tokens"].append(
        {
            "id": "mcp-reader",
            "name": "MCP read-only",
            "enabled": True,
            "token_sha256": hashlib.sha256(token.encode()).hexdigest(),
            "scopes": ["read:" + prefix],
        }
    )
    values = {
        "TOKEN_REGISTRY_JSON": json.dumps(registry, separators=(",", ":")),
        "BRAINS_MCP_API_TOKEN": token,
        "BRAINS_MCP_ACCESS_TOKEN": secrets.token_urlsafe(32),
    }
    lines = []
    for line in path.read_text().splitlines():
        key = line.partition("=")[0].strip()
        if key in values:
            lines.append(f"{key}='{values.pop(key)}'")
        else:
            lines.append(line)
    lines.extend(f"{key}='{value}'" for key, value in values.items())
    path.write_text("\n".join(lines) + "\n")
    path.chmod(0o600)
    print(
        "Read-only MCP credentials saved privately. Deploy the updated core registry before the MCP Worker."
    )


def deploy_mcp(env, dry_run=False):
    for key in (
        "BRAINS_MCP_API_TOKEN",
        "BRAINS_MCP_ACCESS_TOKEN",
        "BRAINS_WORKER_NAME",
        "TOKEN_REGISTRY_JSON",
    ):
        if not env.get(key):
            raise ValueError(f"Configure {key} before deploying MCP")
    digest = hashlib.sha256(env["BRAINS_MCP_API_TOKEN"].encode()).hexdigest()
    tokens = json.loads(env["TOKEN_REGISTRY_JSON"])["tokens"]
    matching = [
        t for t in tokens if t.get("enabled") and t.get("token_sha256") == digest
    ]
    if not matching or any(
        not scope.startswith("read:") for t in matching for scope in t["scopes"]
    ):
        raise ValueError("The MCP upstream token must have read scopes only")
    root = ROOT / "apps/mcp"
    config = tomllib.loads((root / "wrangler.toml").read_text())
    config["main"] = str(root / config["main"])
    config["name"] = (
        env.get("BRAINS_MCP_WORKER_NAME") or env["BRAINS_WORKER_NAME"] + "-mcp"
    )
    config["services"][0]["service"] = env["BRAINS_WORKER_NAME"]
    if env.get("BRAINS_MCP_HOSTNAME"):
        config.update(workers_dev=False, preview_urls=False)
        config["routes"] = [
            {"pattern": env["BRAINS_MCP_HOSTNAME"], "custom_domain": True}
        ]
    with tempfile.TemporaryDirectory(prefix="brains-mcp-") as directory:
        path = Path(directory)
        (path / "wrangler.json").write_text(json.dumps(config))
        secret_path = path / "secrets.json"
        secret_path.write_text(
            json.dumps(
                {
                    "MCP_ACCESS_TOKEN": env["BRAINS_MCP_ACCESS_TOKEN"],
                    "BRAINS_API_TOKEN": env["BRAINS_MCP_API_TOKEN"],
                }
            )
        )
        secret_path.chmod(0o600)
        command = [
            "pnpm",
            "--dir",
            str(root),
            "exec",
            "wrangler",
            "deploy",
            "--config",
            str(path / "wrangler.json"),
            "--secrets-file",
            str(secret_path),
        ]
        if dry_run:
            command += ["--dry-run", "--outdir", str(path / "bundle")]
        subprocess.run(command, cwd=ROOT, env=env, check=True)
