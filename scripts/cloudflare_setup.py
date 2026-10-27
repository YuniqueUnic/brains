"""Provision dedicated upload-only AI Search resources using the official API."""

import json
import urllib.error
import urllib.parse
import urllib.request

from cloudflare_config import structured_setting


def provision(env):
    for name in ("CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"):
        if not env.get(name):
            raise ValueError(
                f"{name} is required for provisioning; Wrangler OAuth remains sufficient for deployment"
            )
    services = structured_setting(env, "BRAINS_CLOUD_CONFIG") or {}
    semantic = services.get("semantic")
    if not semantic:
        raise ValueError("Configure semantic.instance before provisioning")
    gateway = env.get("BRAINS_AI_GATEWAY_ID", "brains-notes")
    account = urllib.parse.quote(env["CLOUDFLARE_ACCOUNT_ID"], safe="")
    namespace = urllib.parse.quote(
        env.get("BRAINS_AI_SEARCH_NAMESPACE", "default"), safe=""
    )
    instance = urllib.parse.quote(semantic["instance"], safe="")
    base = f"https://api.cloudflare.com/client/v4/accounts/{account}"

    def request(method, path, body=None, missing=False):
        req = urllib.request.Request(
            base + path,
            method=method,
            headers={
                "Authorization": "Bearer " + env["CLOUDFLARE_API_TOKEN"],
                "Content-Type": "application/json",
            },
            data=None if body is None else json.dumps(body).encode(),
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as response:
                data = json.load(response)
        except urllib.error.HTTPError as error:
            if missing and error.code == 404:
                return None
            raise ValueError(
                f"Cloudflare provisioning {method} {path}: HTTP {error.code}; verify AI Search and AI Gateway permissions"
            ) from error
        if not data.get("success"):
            raise ValueError(
                f"Cloudflare rejected {method} {path}; inspect the resource in the dashboard"
            )
        return data["result"]

    gateway_path = "/ai-gateway/gateways/" + urllib.parse.quote(gateway, safe="")
    existing = request("GET", gateway_path, missing=True)
    if existing is None:
        request(
            "POST",
            "/ai-gateway/gateways",
            {
                "id": gateway,
                "collect_logs": False,
                "cache_ttl": 0,
                "cache_invalidate_on_update": True,
                "rate_limiting_interval": 0,
                "rate_limiting_limit": 0,
            },
        )
    elif (
        existing.get("cache_ttl")
        or existing.get("rate_limiting_limit")
        or existing.get("collect_logs")
    ):
        raise ValueError(
            "The existing Gateway has caching, payload logging or rate limits; use a dedicated Gateway or review its settings"
        )
    path = f"/ai-search/namespaces/{namespace}/instances"
    existing = request("GET", path + "/" + instance, missing=True)
    model = semantic.get("embedding_model", "@cf/qwen/qwen3-embedding-0.6b")
    if existing is None:
        request(
            "POST",
            path,
            {
                "id": semantic["instance"],
                "embedding_model": model,
                "ai_gateway_id": gateway,
                "cache": False,
                "score_threshold": 0,
                "rewrite_query": False,
                "reranking": False,
                "public_endpoint_params": {"enabled": False},
            },
        )
    elif (
        existing.get("embedding_model") != model
        or existing.get("source")
        or existing.get("ai_gateway_id") != gateway
        or existing.get("public_endpoint_params", {}).get("enabled")
    ):
        raise ValueError(
            "The existing instance has a different model or data source; use a dedicated upload-only instance"
        )
    print(
        "AI Search and Gateway are configured. Public search is disabled; no notes were uploaded."
    )
