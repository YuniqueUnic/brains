"""Collect explicit provider gauges without changing application usage tables."""

import datetime as dt
import json
import math
from pathlib import Path
from urllib.parse import quote

from cloudflare_api import CloudflareApi
from cloudflare_config import structured_setting


def account_storage(env, api=None):
    api = api or CloudflareApi(env)
    profile = structured_setting(env, "BRAINS_RESOURCE_PROFILE")
    if profile is None:
        profile = json.loads((Path(__file__).resolve().parent.parent / "crates/domain/resources/cloudflare-free-2026-10-27.json").read_text())
    rules = [rule for rule in profile["rules"] if
             rule["resource"]["service"] == "d1" and rule["measure"]["metric"] == "storage"
             and rule["scope"]["kind"] == "account" and rule["window"]["kind"] == "none"
             and rule["limit"]["kind"] == "allowance"]
    snapshot = {"account": env.get("BRAINS_RESOURCE_ACCOUNT") or api.account, "observations": []}
    if not rules:
        return snapshot
    if len(rules) != 1:
        raise ValueError("Account storage observation requires one matching D1 gauge rule")
    ttl = int(env.get("BRAINS_OBSERVATION_TTL_SECONDS", "86400"))
    if ttl <= 0:
        raise ValueError("BRAINS_OBSERVATION_TTL_SECONDS must be positive")
    total = 0
    known = set()
    at = int(dt.datetime.now(dt.timezone.utc).timestamp())
    # D1 inventory is a control-plane read, not a SQL query. Exhaust pagination
    # once per explicit deployment; repeated pages fail instead of looping.
    for page in range(1, 101):
        rows = api.request("GET", f"/d1/database?per_page=100&page={page}")
        for row in rows:
            identifier = row["uuid"]
            if not identifier or identifier in known:
                raise ValueError("D1 inventory contains missing or repeated database identifiers")
            # List responses do not expose storage. The details endpoint does.
            detail = api.request("GET", "/d1/database/" + quote(identifier, safe=""))
            amount = detail.get("file_size")
            if type(amount) not in (int, float) or not math.isfinite(amount) or amount < 0:
                raise ValueError("D1 inventory is incomplete or contains invalid storage measurements")
            known.add(identifier)
            total += amount
        if len(rows) < 100:
            break
    else:
        raise ValueError("D1 inventory exceeds the bounded observation page limit")
    snapshot["observations"] = [{
            "resource": rules[0]["resource"],
            "measure": {"metric": "storage", "unit": "byte"}, "amount": total,
            "evidence": "exact", "observed_at": at, "expires_at": at + ttl,
            "source": "cloudflare.d1.inventory",
        }]
    return snapshot
