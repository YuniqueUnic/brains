#!/usr/bin/env python3
"""Real isolated D1/R2 journeys with the local managed-provider fixture."""

import json
import datetime
import os
import urllib.error
import urllib.parse
import urllib.request

base = os.environ["BRAINS_BASE_URL"].rstrip("/")
token = os.environ["BRAINS_E2E_TOKEN"]


def request(method, path, body=None, headers=None, authenticated=True, expected=200):
    values = {"Authorization": "Bearer " + token} if authenticated else {}
    values.update(headers or {})
    if isinstance(body, dict):
        body = json.dumps(body).encode()
        values["Content-Type"] = "application/json"
    req = urllib.request.Request(base + path, data=body, method=method, headers=values)
    try:
        response = urllib.request.urlopen(req, timeout=45)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        value = json.loads(raw) if raw and "application/json" in response.headers.get("Content-Type", "") else raw
        assert response.status == expected, (method, path, response.status, value)
        return value, response.headers


def control(path="stats", body=None):
    return request("POST" if body is not None or path in ("tick", "arm") else "GET",
                   "/_cost/" + path, body)[0]


def route(path, trash=False):
    return "/api/v1/" + ("trash" if trash else "file") + "?" + urllib.parse.urlencode({"path": path})


def put(path, text):
    return request("PUT", route(path), text.encode(),
                   {"Content-Type": "text/markdown", "If-None-Match": "*"})[0]


def states(snapshot, path):
    return [job["state"] for job in snapshot["jobs"] if job["path"] == path]


def consumed(snapshot, rule):
    return next((row["consumed"] for row in snapshot["usage"] if row["rule"] == rule), 0)


# Give this disposable vault an explicit test billing window; no cloud account
# or bill is involved. Unenforced mode permits the fake managed-ingestion cost.
settings = request("GET", "/api/v1/resources")[0]["settings"]
start = datetime.datetime.now(datetime.timezone.utc).replace(day=1, hour=0, minute=0, second=0, microsecond=0)
end = start.replace(year=start.year + (start.month == 12), month=start.month % 12 + 1)
request("PUT", "/api/v1/resources/settings", {
    "expected_revision": settings["revision"], "enforced": False,
    "policy": settings["policy"], "rules": [],
    "billing_cycles": [{"id": "r2", "start": int(start.timestamp()), "end": int(end.timestamp())}],
})

# Even a valid browser session must pass its Origin/CSRF gate before D1 setup.
_, headers = request("POST", "/api/v1/session")
cookie = headers["Set-Cookie"].split(";", 1)[0]
before = control()
request("GET", "/healthz", authenticated=False)
request("HEAD", "/healthz", authenticated=False)
request("GET", route("missing.md"), authenticated=False, expected=401)
request("POST", "/api/v1/unknown", b"{}", authenticated=False, expected=401)
request("GET", "/api/v1/resources", headers={"Authorization": "Bearer invalid"}, expected=401)
request("PUT", route("forbidden.md"), b"reject", authenticated=False,
        headers={"Cookie": cookie, "Origin": base, "x-csrf-token": "wrong"}, expected=403)
after = control()
assert after["usage"] == before["usage"] and after["calls"] == before["calls"]
print("health and early credential/Origin/CSRF rejection: zero ledger/provider amplification", flush=True)

before = control()
request("POST", "/api/v1/files/query", {"source": "store", "prefix": "empty/", "limit": 1})
after = control()
assert consumed(after, "r2-class-b") == consumed(before, "r2-class-b")
assert consumed(after, "d1-writes") - consumed(before, "d1-writes") == 64
print("empty authoritative page: no phantom R2 read or third ledger charge", flush=True)

put("cost/live.md", "# Live source\nVersioned content remains authoritative.")
control("provider", {"existingStatus": "error"})
before = control("arm")
request("POST", "/api/v1/unknown", {}, expected=404)
assert control()["calls"] == before["calls"]
failed = control("tick")
assert states(failed, "cost/live.md") == ["failed"]
assert failed["calls"]["upload"] == 1 and failed["calls"]["delete"] == 0
for _ in range(3):
    stopped = control("tick")
    assert stopped["calls"] == failed["calls"]
print("failed response does not dispatch; rejected managed item stops without automatic reupload", flush=True)

control("provider", {"status": "completed"})
request("POST", "/api/v1/maintenance/reindex", {"limit": 2})
ready = control("tick")
assert states(ready, "cost/live.md") == ["ready"], ready
assert ready["calls"]["upload"] == 2 and ready["calls"]["delete"] == 1
print("explicit maintenance resets one failed provider item and resumes to ready", flush=True)

created = put("cost/remove.md", "# Delete me\nNo background resurrection.")
control("provider", {"deleteFails": True})
trashed, _ = request("DELETE", route("cost/remove.md"), headers={"If-Match": '"' + created["revision"] + '"'})
request("DELETE", route("cost/remove.md", trash=True), headers={"If-Match": '"' + trashed["revision"] + '"'})
paused = control()
assert states(paused, "cost/remove.md") == ["delete_failed"]
control("provider", {"deleteFails": False})
request("POST", "/api/v1/maintenance/reindex", {"limit": 2})
for _ in range(3):
    cleaned = control("tick")
assert states(cleaned, "cost/remove.md") == ["deleted"], cleaned
assert not any(item["key"] == "p1/" + created["revision"] + ".md" for item in cleaned["items"])
request("GET", route("cost/remove.md"), expected=404)
print("purged-source cleanup failure pauses, then explicit bounded retry deletes the provider object", flush=True)

# Manual mode queues current sources and cleans removals without publishing.
control("publication", {"mode": "manual"})
before = control()
manual = put("manual/current.md", "# Manual source\nUpload only on explicit maintenance.")
assert control()["calls"]["upload"] == before["calls"]["upload"]
assert states(control("tick"), "manual/current.md") == ["pending"]
report = request("POST", "/api/v1/maintenance/semantic/reconcile", {"limit": 0})[0]
assert report["processed"] == 0 and report["states"]["pending"] == 1
request("POST", "/api/v1/maintenance/semantic/reconcile", {"limit": 3}, expected=400)
request("POST", "/api/v1/maintenance/semantic/reconcile", {"limit": 1}, authenticated=False, expected=401)
control("arm")
request("POST", "/api/v1/maintenance/semantic/reconcile", {"limit": 2})
assert control()["calls"]["upload"] == before["calls"]["upload"] + 1
control("arm")
ready = request("POST", "/api/v1/maintenance/semantic/reconcile", {"limit": 2})[0]
assert ready["states"]["ready"] >= 1
assert states(control(), "manual/current.md") == ["ready"]
# A delayed deletion meeting the live source is requeued, never uploaded.
control("invalidate-live", {"path": "manual/current.md"})
before = control()
after = control("tick")
assert states(after, "manual/current.md") == ["pending"]
assert before["calls"] == after["calls"]
updated = request("PUT", route("manual/current.md"), b"Updated manual source",
                  {"Content-Type": "text/markdown", "If-Match": '\"' + manual["revision"] + '\"'})[0]
assert control()["calls"]["upload"] == before["calls"]["upload"]
assert sorted(states(control(), "manual/current.md")) == ["deleted", "pending"]
request("DELETE", route("manual/current.md"), headers={"If-Match": '\"' + updated["revision"] + '\"'})
assert states(control("tick"), "manual/current.md") == ["deleted", "deleted"]
control("publication", {"mode": "automatic"})
print("manual publication: bounded explicit refresh, read-only progress, no automatic upload, deletion-only cleanup", flush=True)

for start in range(0, 1001, 50):
    control("seed-scan", {"start": start, "count": min(50, 1001 - start)})
before = control()
req = urllib.request.Request(base + "/api/v1/files/query", method="POST",
    data=json.dumps({"source": "store", "prefix": "cost-scan/", "limit": 1}).encode(),
    headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
try:
    urllib.request.urlopen(req, timeout=45)
    raise AssertionError("truncated authoritative scan was reported as successful")
except urllib.error.HTTPError as error:
    with error:
        assert error.code >= 500
        assert b"R2_LIST_PAGE_MAX" in error.read()
after = control()
assert consumed(after, "r2-class-a") - consumed(before, "r2-class-a") == 1
assert consumed(after, "r2-class-b") == consumed(before, "r2-class-b")
plans = control("plans")["plans"]
assert any("semantic_items_work" in plan for plan in plans), plans
assert any("semantic_items_paused" in plan for plan in plans), plans
assert not any("TEMP B-TREE" in plan for plan in plans), plans
print("R2 scan cap fails explicitly; active/paused work uses ordered partial indexes", flush=True)
control("settings-offline", {})
try:
    request("GET", "/healthz", authenticated=False)
    request("GET", "/api/v1/capabilities", authenticated=False, expected=401)
    request("PUT", route("forbidden.md"), b"reject", authenticated=False,
            headers={"Cookie": cookie, "Origin": base, "x-csrf-token": "wrong"}, expected=403)
    request("GET", "/api/v1/capabilities", expected=500)
finally:
    control("settings-online", {})
print("unavailable settings backend: health/early auth still work; authenticated setup fails as positive control", flush=True)
print("CLOUDFLARE COST WORKFLOWS PASS", flush=True)
