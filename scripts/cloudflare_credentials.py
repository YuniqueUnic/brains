"""Synchronize the configured owner token without rotating instance keys."""

import hashlib
import json
import os


def sync_admin_token(path, env):
    if os.environ.get("GITHUB_ACTIONS") == "true":
        raise ValueError("Synchronize the owner token locally before uploading secrets")
    token = env.get("BRAINS_ADMIN_TOKEN", "")
    if not token or not all(33 <= ord(character) <= 126 for character in token):
        raise ValueError(
            "BRAINS_ADMIN_TOKEN must be nonempty printable ASCII without whitespace"
        )
    registry = json.loads(env.get("TOKEN_REGISTRY_JSON") or "{}")
    digest = hashlib.sha256(token.encode()).hexdigest()
    tokens = registry.get("tokens", [])
    if any(
        t.get("enabled")
        and "admin:/" in t.get("scopes", [])
        and t.get("token_sha256") == digest
        for t in tokens
    ):
        print("Configured admin token already matches the enabled registry.")
        return
    owners = [
        t
        for t in tokens
        if t.get("id") == "owner"
        and t.get("enabled")
        and "admin:/" in t.get("scopes", [])
    ]
    if len(owners) != 1:
        raise ValueError("Custom registry: update the intended admin hash explicitly")
    owners[0]["token_sha256"] = digest
    encoded = json.dumps(registry, separators=(",", ":"))
    if "'" in encoded:
        raise ValueError(
            "Custom registry contains a quote requiring manual dotenv encoding"
        )
    lines = [
        line
        for line in path.read_text().splitlines()
        if line.partition("=")[0].strip() != "TOKEN_REGISTRY_JSON"
    ]
    lines.append("TOKEN_REGISTRY_JSON='" + encoded + "'")
    path.write_text("\n".join(lines) + "\n")
    path.chmod(0o600)
    env["TOKEN_REGISTRY_JSON"] = encoded
    print(
        "Owner token hash synchronized privately; signing keys and other principals retained."
    )
