#!/usr/bin/env python3
"""Verify deployment-time provider observations with an offline API fixture."""

import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
from cloudflare_config import compose_services
from cloudflare_observations import account_storage


class Api:
    account = "fixture-account"

    def __init__(self, count=2, size=1_000_000, repeat=False):
        self.count, self.size, self.repeat = count, size, repeat
        self.calls = []

    def request(self, method, path):
        assert method == "GET" and "/query" not in path
        self.calls.append(path)
        if "?" in path:
            page = int(path.rsplit("=", 1)[1])
            start = 0 if self.repeat else (page - 1) * 100
            return [{"uuid": f"database-{n}"} for n in range(start, min(start + 100, self.count))]
        return {"file_size": self.size}


for count in (0, 2, 101):
    api = Api(count)
    snapshot = account_storage({}, api)
    sample = snapshot["observations"][0]
    assert sample["amount"] == count * 1_000_000
    assert sample["expires_at"] - sample["observed_at"] == 86400
    assert sample["measure"] == {"metric": "storage", "unit": "byte"}
    assert len(api.calls) == count + (count // 100 + 1)
    config = {"vars": {}}
    compose_services(config, {"CLOUDFLARE_ACCOUNT_ID": api.account,
                              "BRAINS_RESOURCE_OBSERVATIONS": json.dumps(snapshot)})
    assert json.loads(config["vars"]["BRAINS_RESOURCE_OBSERVATIONS"]) == snapshot

for size in (None, -1, True, float("nan"), float("inf")):
    try:
        account_storage({}, Api(size=size))
        raise AssertionError("Invalid size became a measurement")
    except ValueError as error:
        assert "invalid storage" in str(error)

try:
    account_storage({}, Api(count=100, repeat=True))
    raise AssertionError("A repeating inventory page was accepted")
except ValueError as error:
    assert "repeated" in str(error)

for ttl in ("0", "-1", "invalid"):
    try:
        account_storage({"BRAINS_OBSERVATION_TTL_SECONDS": ttl}, Api())
        raise AssertionError("Invalid expiry was accepted")
    except ValueError:
        pass
sample = account_storage({"BRAINS_OBSERVATION_TTL_SECONDS": "60"}, Api())["observations"][0]
assert sample["expires_at"] - sample["observed_at"] == 60
try:
    compose_services({"vars": {}}, {"CLOUDFLARE_ACCOUNT_ID": "other",
                                   "BRAINS_RESOURCE_OBSERVATIONS": json.dumps(snapshot)})
    raise AssertionError("A different account snapshot was accepted")
except ValueError as error:
    assert "account" in str(error)

api = Api()
assert account_storage({"BRAINS_RESOURCE_PROFILE": '{"rules":[]}'}, api)["observations"] == []
assert api.calls == []
print("Provider observations: complete pagination, details, expiry and identity PASS")
