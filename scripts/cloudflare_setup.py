"""Provision dedicated upload-only AI Search resources using the official API."""

import urllib.parse

from cloudflare_api import CloudflareApi
from cloudflare_config import structured_setting

def provision(env):
    request = CloudflareApi(env).request
    services = structured_setting(env, "BRAINS_CLOUD_CONFIG") or {}
    semantic = services.get("semantic")
    if not semantic:
        raise ValueError("Configure semantic.instance before provisioning")
    gateway = env.get("BRAINS_AI_GATEWAY_ID", "brains-notes")
    namespace = urllib.parse.quote(
        env.get("BRAINS_AI_SEARCH_NAMESPACE", "default"), safe=""
    )
    instance = urllib.parse.quote(semantic["instance"], safe="")
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
    model = semantic.get("embedding_model", "@cf/baai/bge-m3")
    if existing is None:
        request(
            "POST",
            path,
            {
                "id": semantic["instance"],
                "embedding_model": model,
                "index_method": {"vector": True, "keyword": False},
                "chunk_size": 512,
                "chunk_overlap": 0,
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
        or existing.get("index_method") != {"vector": True, "keyword": False}
        or existing.get("chunk_size") != 512
        or existing.get("chunk_overlap") != 0
        or existing.get("rewrite_query")
        or existing.get("reranking")
        or existing.get("cache")
        or existing.get("score_threshold") != 0
        or existing.get("ai_gateway_id") != gateway
        or existing.get("public_endpoint_params", {}).get("enabled")
    ):
        raise ValueError(
            "The existing instance has a different model or data source; use a dedicated upload-only instance"
        )
    print(
        "AI Search and Gateway are configured. Public search is disabled; no notes were uploaded."
    )
