#!/usr/bin/env python3
"""Exercise recoverable Worker failures with real, isolated workerd D1."""

import http.client
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request

base = os.environ["BRAINS_BASE_URL"].rstrip("/")
origin = urllib.parse.urlsplit(base)
assert origin.hostname in ("127.0.0.1", "localhost", "::1"), "Local fixtures only"
token = os.environ["BRAINS_E2E_TOKEN"]


def request(method, path, body=None, expected=200, headers=None):
    headers = {"Authorization": "Bearer " + token, **(headers or {})}
    if isinstance(body, dict):
        body = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(base + path, data=body, method=method, headers=headers)
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        value = json.loads(raw) if "application/json" in response.headers.get("Content-Type", "") else raw
        assert response.status == expected, (path, response.status, value)
        return value


def conserved(before, after):
    for metric, rule in (("rows_read", "d1-reads"), ("rows_written", "d1-writes")):
        total = lambda snapshot: sum(row["consumed"] for row in snapshot["usage"] if row["rule"] == rule)
        actual = sum(row[metric] for row in after["measurements"])
        assert total(after) - total(before) >= actual, (metric, total(after) - total(before), actual)


def closing(snapshot):
    return [row for row in snapshot["measurements"] if row["operation"] == "batch"
            and (row["sql"].startswith("UPDATE resource_usage SET consumed=consumed+")
                 or row["sql"].startswith("INSERT INTO resource_usage") and "VALUES (" in row["sql"])]


def wait_for(predicate):
    end = time.monotonic() + 10
    while time.monotonic() < end:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError("Local operation did not finish before its deadline")


initial = request("GET", "/api/v1/resources")
request("PUT", "/api/v1/resources/settings", {
    "expected_revision": initial["settings"]["revision"], "enforced": False,
    "policy": initial["settings"]["policy"], "billing_cycles": [], "rules": [],
})
request("PUT", "/api/v1/file?path=failures/seed.md", b"# Measured failures\n",
        headers={"Content-Type": "text/markdown", "If-None-Match": "*"})

# Configuration failures after reading saved settings still finalize that read.
before = request("GET", "/_ledger/start")
request("POST", "/_ledger/control", {"overrides": {"BRAINS_CLOUD_CONFIG": '{"url_import":true}'}})
request("GET", "/api/v1/resources", expected=500)
after = request("GET", "/_ledger/snapshot")
conserved(before, after)
assert len(closing(after)) == 2
request("POST", "/_ledger/control", {})
request("POST", "/_ledger/profile", {"invalid": True})
before = request("GET", "/_ledger/start")
request("GET", "/api/v1/resources", expected=500)
after = request("GET", "/_ledger/snapshot")
conserved(before, after)
assert len(closing(after)) == 2
request("POST", "/_ledger/profile", {"invalid": False})
print("Failed optional service and corrupt saved profile: finalized", flush=True)

# Disconnect after application SQL commits but before its receipt is returned.
before = request("GET", "/_ledger/start")
request("POST", "/_ledger/control", {"holdSQL": "INSERT INTO files"})
connection = http.client.HTTPConnection(origin.hostname, origin.port, timeout=15)
connection.request("PUT", "/api/v1/file?path=failures/disconnected.md", b"# Exactly one commit\n", {
    "Authorization": "Bearer " + token, "Content-Type": "text/markdown", "If-None-Match": "*",
})
try:
    wait_for(lambda: request("GET", "/_ledger/control")["held"])
finally:
    connection.close()
after = wait_for(lambda: (snapshot if closing(snapshot := request("GET", "/_ledger/snapshot")) else None))
conserved(before, after)
assert sum("INSERT INTO files" in row["sql"] for row in after["measurements"]) == 1
request("POST", "/_ledger/control", {})
assert b"Exactly one commit" in request("GET", "/api/v1/file?path=failures/disconnected.md")
print("Disconnected response consumer: one business commit and a completed flush", flush=True)

# Each final statement is already covered by its small closing bound. Missing
# final metadata must not replay earlier receipts or the closing batch itself.
before = request("GET", "/_ledger/start")
request("POST", "/_ledger/control", {"fault": {"sql": "UPDATE resource_usage SET consumed=consumed+", "at": 2, "missing": "meta"}})
request("GET", "/api/v1/resources")
after = request("GET", "/_ledger/snapshot")
conserved(before, after)
assert len(closing(after)) == 2
assert request("GET", "/_ledger/control")["faultCount"] == 2
request("POST", "/_ledger/control", {"failClosing": True})
request("GET", "/api/v1/resources", expected=503)
request("POST", "/_ledger/control", {})
print("Partial closing metadata is bounded; failed final commit returns 503", flush=True)

# Application SQL has no universal row bound. Missing metadata is Unknown,
# never an exact zero. The completed operation is not replayed to obtain it.
for missing in ("meta", "rows_read", "rows_written"):
    request("POST", "/_ledger/control", {"fault": {"sql": "FROM files", "missing": missing}})
    request("POST", "/api/v1/files/query", {"source": "index", "limit": 2}, expected=503)
    assert request("GET", "/_ledger/control")["faultCount"] == 1
    request("POST", "/_ledger/control", {})
    report = request("GET", "/api/v1/resources")
    assert all(row["evidence"] == "unknown" for row in report["usage"] if row["rule"] in ("d1-reads", "d1-writes"))
print("Missing application metadata: explicit Unknown evidence, no business replay", flush=True)

at = int(time.time())
observation = {"resource": {"service": "d1", "kind": "database"},
               "measure": {"metric": "storage", "unit": "byte"}, "amount": 3_000_000,
               "evidence": "exact", "observed_at": at, "expires_at": at + 60,
               "source": "local fixture inventory"}
for expired in (False, True):
    sample = {**observation, **({"observed_at": at - 120, "expires_at": at - 60} if expired else {})}
    request("POST", "/_ledger/control", {"overrides": {"BRAINS_RESOURCE_OBSERVATIONS": json.dumps({"account": "deployment", "observations": [sample]})}})
    first = request("GET", "/api/v1/resources")
    second = request("GET", "/api/v1/resources")
    for report in (first, second):
        stored = [row for row in report["usage"] if row["rule"] == "d1-account-storage"]
        assert len(stored) == 1 and stored[0]["consumed"] == 3_000_000
        assert stored[0]["observed_at"] == sample["observed_at"] and stored[0]["expires_at"] == sample["expires_at"]
request("POST", "/_ledger/control", {"overrides": {"BRAINS_RESOURCE_OBSERVATIONS": json.dumps({"account": "wrong", "observations": [observation]})}})
request("GET", "/api/v1/resources", expected=500)
request("POST", "/_ledger/control", {})
assert not any(row["rule"] == "d1-account-storage" for row in request("GET", "/api/v1/resources")["usage"])
print("Account snapshot: timestamped, expiring, non-additive and account-scoped", flush=True)

current = request("GET", "/api/v1/resources")
request("PUT", "/api/v1/resources/settings", {
    "expected_revision": current["settings"]["revision"], "enforced": True,
    "policy": current["settings"]["policy"], "rules": [],
    "billing_cycles": [{"id": cycle["id"], "start": at - 60, "end": at + 86400}
                       for cycle in current["settings"]["billing_cycles"]],
})
request("GET", "/_ledger/start")
request("GET", "/api/v1/file?path=failures/seed.md", expected=503)
assert not any("FROM files" in row["sql"] for row in request("GET", "/_ledger/snapshot")["measurements"])
request("GET", "/api/v1/resources")
print("Unknown measured usage blocks new strict work; administration remains reachable", flush=True)
