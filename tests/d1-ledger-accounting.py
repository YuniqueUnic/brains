#!/usr/bin/env python3
"""Compare ledger deltas with exact metadata from an isolated workerd workload."""

import argparse
import collections
import datetime as dt
import json
import os
from pathlib import Path
import urllib.error
import urllib.parse
import urllib.request
import uuid

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline", action="store_true")
parser.add_argument("--record", type=Path)
args = parser.parse_args()
base = os.environ["BRAINS_BASE_URL"].rstrip("/")
assert urllib.parse.urlsplit(base).hostname in ("127.0.0.1", "localhost", "::1"), "Local fixtures only"
token = os.environ["BRAINS_E2E_TOKEN"]


def request(method, route, body=None, headers=None, expected=200):
    values = {"Authorization": "Bearer " + token, **(headers or {})}
    if isinstance(body, dict):
        body = json.dumps(body).encode()
        values["Content-Type"] = "application/json"
    req = urllib.request.Request(base + route, data=body, method=method, headers=values)
    try:
        response = urllib.request.urlopen(req, timeout=45)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        value = json.loads(raw) if "application/json" in response.headers.get("Content-Type", "") else raw
        assert response.status == expected, (method, route, response.status, value)
        return value


def consumed(snapshot, rule):
    return sum(row["consumed"] for row in snapshot["usage"] if row["rule"] == rule)


def conservation(before, after):
    statements = after["measurements"]
    closing = [row for row in statements if (
        row["sql"].startswith("INSERT INTO resource_usage") and "VALUES (" in row["sql"]
    ) or row["sql"].startswith("UPDATE resource_usage SET consumed=consumed+")]
    groups = collections.defaultdict(list)
    for row in closing:
        groups[row["batch"]].append(row)
    assert groups and all(len(rows) == 2 for rows in groups.values()), groups
    for metric, index, rule in (("rows_read", 0, "d1-reads"), ("rows_written", 1, "d1-writes")):
        allowance = 0
        for rows in groups.values():
            known = rows[0]["sql"].startswith("UPDATE")
            assert all(row["sql"].startswith("UPDATE") == known for row in rows)
            bound = ((2, 2) if known else (4, 10))[index]
            assert sum(row[metric] for row in rows) <= bound, rows
            allowance += bound
        actual = sum(row[metric] for row in statements)
        delta = consumed(after, rule) - consumed(before, rule)
        expected = actual - sum(row[metric] for row in closing) + allowance
        assert delta == expected, (metric, delta, expected, actual)
        assert delta >= actual
    return len(groups)


# Use the unchanged dated catalog and 80% policy. Only disposable fixture
# billing dates are supplied; no usage table is reset or edited.
settings = request("GET", "/api/v1/resources")["settings"]
assert settings["policy"]["fraction"] == 0.8
start = dt.datetime.now(dt.timezone.utc).replace(day=1, hour=0, minute=0, second=0, microsecond=0)
end = start.replace(year=start.year + (start.month == 12), month=start.month % 12 + 1)
request("PUT", "/api/v1/resources/settings", {
    "expected_revision": settings["revision"], "enforced": True,
    "policy": settings["policy"], "rules": [],
    "billing_cycles": [{"id": cycle["id"], "start": int(start.timestamp()), "end": int(end.timestamp())}
                       for cycle in settings["billing_cycles"]],
})
before = request("GET", "/_ledger/start")
counts = collections.Counter()
for number in range(3):
    route = "/api/v1/file?" + urllib.parse.urlencode({"path": f"ledger/note-{number}.md"})
    created = request("PUT", route, f"# Ledger note {number}\nFixed accounting workload.\n".encode(),
                      {"Content-Type": "text/markdown", "If-None-Match": "*"})
    counts["create"] += 1
    request("PUT", route, f"# Ledger note {number}\nUpdated accounting workload.\n".encode(),
            {"Content-Type": "text/markdown", "If-Match": '"' + created["revision"] + '"'})
    counts["update"] += 1
for number in range(6):
    route = "/api/v1/file?" + urllib.parse.urlencode({"path": f"ledger/note-{number % 3}.md"})
    request("GET", route)
    counts["read"] += 1
    request("POST", "/api/v1/files/query", {"source": "index", "limit": 3})
    counts["index_query"] += 1
for mode, count in (("fts", 4), ("hybrid", 3)):
    for _ in range(count):
        result = request("POST", "/api/v1/search", {"q": "accounting", "mode": mode})
        assert result["items"] and not result["unavailable"]
        counts[mode + "_query"] += 1
published = request("GET", "/api/v1/resources")
counts["resources_report"] += 1
after = request("GET", "/_ledger/snapshot")
assert after["measurements"], "No D1 statements were measured"
assert all(isinstance(row.get(key), int) for row in after["measurements"]
           for key in ("rows_read", "rows_written")), "Simulator metadata is unavailable"
report = {"environment": "isolated local workerd, real D1/R2, no-network AI Search fixture",
          "workload": dict(counts), "requests": sum(counts.values()), "ratios": {},
          "inspection_sql_excluded": True, "statements": after["measurements"],
          "published_usage": published["usage"], "before_usage": before["usage"], "after_usage": after["usage"]}
for metric, rule in (("rows_read", "d1-reads"), ("rows_written", "d1-writes")):
    actual = sum(row[metric] for row in after["measurements"])
    ledger = consumed(after, rule) - consumed(before, rule)
    assert actual > 0
    report["ratios"][metric] = {"ledger": ledger, "actual": actual, "ratio": ledger / actual}
if args.record:
    args.record.parent.mkdir(parents=True, exist_ok=True)
    args.record.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({key: report[key] for key in ("environment", "workload", "requests", "ratios")}, indent=2))
if args.baseline:
    assert report["ratios"]["rows_written"]["ratio"] > 1, "Over-count was not reproduced"
else:
    for result in report["ratios"].values():
        assert result["actual"] <= result["ledger"] <= 2 * result["actual"], result
    report["closed_scopes"] = conservation(before, after)
    checks = [row for row in after["measurements"] if row["sql"].endswith("AS allowed;")]
    assert checks and all(row["rows_written"] == 0 for row in checks)


report["journeys"] = {}
def journey(label, operation):
    initial = request("GET", "/_ledger/start")
    operation()
    final = request("GET", "/_ledger/snapshot")
    closed = conservation(initial, final) if not args.baseline else None
    report["journeys"][label] = {
        "closed_scopes": closed, "statements": final["measurements"],
        "metrics": {metric: {
            "ledger": consumed(final, rule) - consumed(initial, rule),
            "actual": sum(row[metric] for row in final["measurements"]),
        } for metric, rule in (("rows_read", "d1-reads"), ("rows_written", "d1-writes"))},
    }
    print(label + (": baseline recorded" if args.baseline else ": receipt conservation PASS"), flush=True)
    return initial, final

for label, path in (("resource report", "/api/v1/resources"),
                    ("capabilities", "/api/v1/capabilities"),
                    ("raw file", "/api/v1/file?path=ledger/note-0.md")):
    initial, final = journey(label + " x10", lambda path=path: [request("GET", path) for _ in range(10)])
    if not args.baseline:
        for metric, rule in (("rows_read", "d1-reads"), ("rows_written", "d1-writes")):
            assert consumed(final, rule) - consumed(initial, rule) == sum(row[metric] for row in final["measurements"])

def browser_read():
    request("GET", "/api/v1/session")
    request("POST", "/api/v1/files/query", {"source": "store", "include_deleted": True, "limit": 100})
    for path in ("/api/v1/capabilities", "/api/v1/file?path=ledger/note-0.md", "/api/v1/resources", "/api/v1/resources"):
        request("GET", path)
journey("browser open, file and usage refresh", browser_read)


if not args.baseline:
    journey("router error", lambda: request("GET", "/api/v1/does-not-exist", expected=404))
    journey("stale file write", lambda: request("PUT", "/api/v1/file?path=ledger/note-0.md", b"stale",
                                               {"Content-Type": "text/markdown", "If-Match": '"' + "0" * 32 + '"'}, expected=412))
    current = request("GET", "/api/v1/resources")

    def update(expected_revision, rules=None, expected=200):
        return request("PUT", "/api/v1/resources/settings", {
            "expected_revision": expected_revision, "enforced": True,
            "policy": current["settings"]["policy"], "rules": rules or [],
            "billing_cycles": current["settings"]["billing_cycles"],
        }, expected=expected)

    journey("settings conflict", lambda: update("0" * 32, expected=412))
    journey("settings save", lambda: update(current["settings"]["revision"]))
    current = request("GET", "/api/v1/resources")
    worker = next(rule for rule in current["rules"] if rule["id"] == "worker-requests")
    worker_update = {"id": worker["id"], "limit": worker["limit"], "policy": {**worker["policy"], "maximum": 0}}
    current = update(current["settings"]["revision"], [worker_update])
    journey("request admission rejected", lambda: request("GET", "/api/v1/file?path=ledger/note-0.md", expected=429))
    update(current["settings"]["revision"], [{**worker_update, "policy": worker["policy"]}])
    for suffix in ("", "?fail=1"):
        initial, final = journey("scheduled reconciliation" + suffix,
                                 lambda suffix=suffix: request("POST", "/_ledger/tick" + suffix, {}))
        assert consumed(initial, "worker-requests") == consumed(final, "worker-requests")
    # Exercise the same terminal SQL against fresh and existing keys, including
    # all indexes from the real migrations. Probes are outside the workload.
    sql = (Path(__file__).resolve().parent.parent / "crates/adapter-d1/src/sql/accounting.sql").read_text()
    charges = [{"key": "closing-probe-" + uuid.uuid4().hex, "meter": "probe-" + metric,
                "rule": "probe-" + metric, "measure": {"metric": metric, "unit": "row"},
                "amount": 0, "limit": 100, "window": {"start": 0, "end": 4102444800},
                "evidence": "bounded", "action": "observe", "mode": "increment"}
               for metric in ("rows_read", "rows_written")]
    report["closing_cost_probe"] = request("POST", "/_ledger/probe-closing", {"sql": sql, "charges": charges})
    for cost in (report["closing_cost_probe"]["fresh"], report["closing_cost_probe"]["existing"]):
        assert cost["rows_read"] <= 4 and cost["rows_written"] <= 10, cost

    update_sql = (Path(__file__).resolve().parent.parent / "crates/adapter-d1/src/sql/accounting-update.sql").read_text()
    report["mixed_closing_probes"] = []
    for index in (0, 1):
        mixed = [{**charge, "key": "mixed-" + uuid.uuid4().hex} for charge in charges]
        result = request("POST", "/_ledger/probe-closing", {
            "sql": sql, "charges": mixed, "seedSql": sql, "seed": [mixed[index]],
        })
        assert result["fresh"]["rows_read"] <= 4 and result["fresh"]["rows_written"] <= 10, result
        report["mixed_closing_probes"].append(result)
    report["known_closing_probes"] = []
    for held in (False, True):
        known = [{**charge, "key": "known-" + uuid.uuid4().hex} for charge in charges]
        result = request("POST", "/_ledger/probe-closing", {
            "sql": update_sql, "charges": known, "seedSql": sql, "seed": known, "held": held,
        })
        assert result["fresh"] == result["existing"] == {"rows_read": 2, "rows_written": 2}, result
        assert result["reserved"] == (6 if held else 0), result
        report["known_closing_probes"].append(result)

    print(json.dumps({"closing_cost_probe": report["closing_cost_probe"], "closed_scopes": report["closed_scopes"]}))
if args.record:
    args.record.write_text(json.dumps(report, indent=2) + "\n")
print("D1 ledger workload: PASS", flush=True)
