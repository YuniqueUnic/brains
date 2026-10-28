#!/usr/bin/env python3
"""Real isolated D1/R2 resource settings, quota recovery and policy invariants."""

import concurrent.futures
import copy
import json
import os
import time
import urllib.error
import urllib.request

BASE = os.environ["BRAINS_BASE_URL"]
OWNER = os.environ["BRAINS_E2E_TOKEN"]
READER = os.environ["BRAINS_E2E_READER_TOKEN"]


def request(method, path, body=None, token=OWNER, headers=None):
    prepared = {"Authorization": "Bearer " + token, **(headers or {})}
    if isinstance(body, dict):
        body = json.dumps(body).encode()
        prepared["Content-Type"] = "application/json"
    req = urllib.request.Request(
        BASE + path, data=body, headers=prepared, method=method
    )
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        data = response.read()
        value = (
            json.loads(data)
            if data and "application/json" in response.headers.get("Content-Type", "")
            else data
        )
        return response.status, value


def current():
    status, value = request("GET", "/api/v1/resources")
    assert status == 200, (status, value)
    return value


def body(report, **patch):
    return {
        "expected_revision": report["settings"]["revision"],
        "enforced": report["settings"]["enforced"],
        "policy": report["settings"]["policy"],
        "billing_cycles": report["settings"]["billing_cycles"],
        "rules": [],
        **patch,
    }


def save(update, expected=200):
    status, value = request("PUT", "/api/v1/resources/settings", update)
    assert status == expected, (status, value)
    return value


initial = current()
assert not initial["settings"]["enforced"] and len(initial["rules"]) == 50
assert any(
    item["rule"] == "worker-requests" and item["consumed"] > 0
    for item in initial["usage"]
)
assert not any(
    item["rule"] in ("r2-class-a", "r2-class-b") for item in initial["usage"]
)
assert request("GET", "/api/v1/resources", token=READER)[0] == 403
assert (
    request("PUT", "/api/v1/resources/settings", body(initial), token=READER)[0] == 403
)
first = save(body(initial, policy={"fraction": 0.5}))
assert first["settings"]["source"] == "saved"
assert current()["settings"]["revision"] == first["settings"]["revision"]
save(body(initial), expected=412)
save(body(first, policy={"when_exhausted": "not-an-action"}), expected=400)
assert current()["settings"]["revision"] == first["settings"]["revision"]
print(
    "Admin-only writes, durable saved policy, stale revisions and invalid-policy 400: PASS",
    flush=True,
)

snapshot = current()
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    futures = [
        pool.submit(
            request,
            "PUT",
            "/api/v1/resources/settings",
            body(snapshot, policy={"fraction": f}),
        )
        for f in (0.6, 0.7)
    ]
    results = [f.result() for f in futures]
assert sorted(status for status, _ in results) == [200, 412], results
print(
    "Concurrent editors: exactly one update wins and the other receives 412: PASS",
    flush=True,
)

now = int(time.time())
report = current()
cycles = copy.deepcopy(report["settings"]["billing_cycles"])
for cycle in cycles:
    if cycle["id"] == "r2":
        cycle.update(start=now - 3600, end=now + 3600)
report = save(
    body(report, enforced=True, billing_cycles=cycles, policy={"fraction": 0.8})
)
status, source = request(
    "PUT",
    "/api/v1/file?path=resource-proof.md",
    b"# Resource proof\n",
    headers={"If-None-Match": "*", "Content-Type": "text/markdown"},
)
assert status == 200, (status, source)
report = current()
assert any(
    item["rule"] == "r2-class-a" and item["consumed"] > 0 for item in report["usage"]
)
changed = copy.deepcopy(report["settings"]["billing_cycles"])
for cycle in changed:
    if cycle["id"] == "r2":
        cycle["end"] += 1
save(body(report, billing_cycles=changed), expected=400)
assert current()["settings"]["revision"] == report["settings"]["revision"]
print(
    "Real source I/O is metered; editing an accounted billing boundary cannot reset allowance: PASS",
    flush=True,
)

worker = next(rule for rule in report["rules"] if rule["id"] == "worker-requests")
report = save(
    body(
        report,
        rules=[
            {"id": worker["id"], "limit": worker["limit"], "policy": {"maximum": 0}}
        ],
    )
)
assert request("GET", "/api/v1/file?path=resource-proof.md")[0] == 429
assert request("GET", "/api/v1/session")[0] == 200
assert request("GET", "/api/v1/capabilities")[0] == 200
assert request("GET", "/healthz")[0] == 200
for path in ("/api/v1/session", "/api/v1/capabilities", "/api/v1/resources"):
    assert request("GET", path, token=READER)[0] == 429, path
status, session = request(
    "POST", "/api/v1/session", {"token": OWNER}, headers={"Origin": BASE}
)
assert status == 200, (status, session)
report = current()
report = save(
    body(report, rules=[{"id": worker["id"], "limit": worker["limit"], "policy": {}}])
)
assert request("GET", "/api/v1/file?path=resource-proof.md")[0] == 200
print(
    "Zero budget blocks readers while health and owner login/repair remain available: PASS",
    flush=True,
)

report = save(
    body(
        current(),
        enforced=False,
        rules=[
            {"id": worker["id"], "limit": worker["limit"], "policy": {"maximum": 0}}
        ],
    )
)
assert not current()["settings"]["enforced"]
usage_before = {item["rule"]: item["consumed"] for item in current()["usage"]}
assert request("GET", "/api/v1/file?path=resource-proof.md")[0] == 200
usage_after = {item["rule"]: item["consumed"] for item in current()["usage"]}
for rule in ("worker-requests", "r2-class-b"):
    assert usage_after[rule] > usage_before[rule], (rule, usage_before, usage_after)
print(
    "Enforcement off still measures usage and admits source I/O above a zero budget: PASS",
    flush=True,
)
print("RESOURCE SETTINGS BUSINESS PASS")
