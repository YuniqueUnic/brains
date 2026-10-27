"""Compose optional Cloudflare services from explicit, portable JSON settings."""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def structured_setting(env, name):
    raw = env.get(name, "")
    filename = env.get(name + "_FILE", "")
    if raw and filename:
        raise ValueError(f"Use {name} or {name}_FILE, not both")
    if filename:
        path = Path(filename)
        raw = (path if path.is_absolute() else ROOT / path).read_text()
    if not raw:
        return None
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError(f"{name} must be a JSON object")
    return value


def compose_services(config, env):
    services = structured_setting(env, "BRAINS_CLOUD_CONFIG")
    profile = structured_setting(env, "BRAINS_RESOURCE_PROFILE")
    if profile is not None:
        for key in ("profile", "rules"):
            if key not in profile:
                raise ValueError(f"Resource profile requires {key}")
        account = env.get("BRAINS_RESOURCE_ACCOUNT") or env.get("CLOUDFLARE_ACCOUNT_ID")
        if not account:
            raise ValueError("Set CLOUDFLARE_ACCOUNT_ID or BRAINS_RESOURCE_ACCOUNT")
        config["vars"]["BRAINS_RESOURCE_ACCOUNT"] = account
        # Each Worker var is limited to 5 KB. ASCII JSON chunks keep byte
        # boundaries explicit without changing the user's profile structure.
        encoded = json.dumps(profile, separators=(",", ":"), ensure_ascii=True)
        chunks = [
            encoded[start : start + 4000] for start in range(0, len(encoded), 4000)
        ]
        if len(chunks) > 32:
            raise ValueError(
                "Resource profiles must fit within 32 parts of 4000 ASCII bytes"
            )
        config["vars"]["BRAINS_RESOURCE_PROFILE_COUNT"] = str(len(chunks))
        for index, chunk in enumerate(chunks):
            config["vars"][f"BRAINS_RESOURCE_PROFILE_{index}"] = chunk
    if services is None:
        return
    if services.get("url_import") and not services.get("document_conversion"):
        raise ValueError("URL import requires document conversion")
    if services.get("document_conversion"):
        config["ai"] = {"binding": "AI"}
    semantic = services.get("semantic")
    if semantic:
        if profile is None:
            raise ValueError(
                "Semantic search requires BRAINS_RESOURCE_PROFILE or its _FILE"
            )
        if not semantic.get("instance"):
            raise ValueError("Semantic search requires an existing instance name")
        config["ai_search_namespaces"] = [
            {
                "binding": "AI_SEARCH",
                "namespace": env.get("BRAINS_AI_SEARCH_NAMESPACE", "default"),
            }
        ]
        config["triggers"] = {
            "crons": [env.get("BRAINS_RECONCILE_CRON", "*/5 * * * *")]
        }
    config["vars"]["BRAINS_CLOUD_CONFIG"] = json.dumps(services, separators=(",", ":"))
