#!/usr/bin/env python3
"""Exercise the production quota SQL with concurrent writers and rollovers."""

import concurrent.futures, json, re, sqlite3, tempfile, time
from pathlib import Path

root = Path(__file__).resolve().parent.parent
fixture = tempfile.TemporaryDirectory(prefix="brains-quota-probe-")
db = Path(fixture.name) / "ledger.sqlite"
connection = sqlite3.connect(db)
connection.executescript(
    (root / "migrations/d1/0003_resources_semantic.sql").read_text()
)
connection.close()
sql = (root / "crates/adapter-d1/src/sql/debit.sql").read_text()
source = (root / "crates/adapter-d1/src/resources.rs").read_text()
reserve = re.findall(r'prepare\(\s*"(INSERT[^"\n]+)"', source)
assert len(reserve) == 3, len(reserve)


def charge(key, amount=1, limit=10, meter=None):
    return dict(
        key=key,
        meter=meter or key,
        rule=key,
        measure=dict(metric="requests", unit="request"),
        amount=amount,
        limit=limit,
        evidence="bounded",
        action="block",
        mode="increment",
        window=dict(start=0, end=4102444800),
    )


def debit(charges, enforce=1):
    with sqlite3.connect(db, timeout=10) as c:
        return c.execute(sql, (json.dumps(charges), enforce)).fetchall()


def usage(key):
    with sqlite3.connect(db) as c:
        return c.execute(
            "SELECT consumed,reserved FROM resource_usage WHERE key=?", (key,)
        ).fetchone()


with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
    results = list(pool.map(lambda _: debit([charge("atomic", 2, 10)]), range(20)))
assert sum(bool(r) for r in results) == 5 and usage("atomic") == (10, 0)
assert (
    not debit([charge("unaffected", 1, 10), charge("atomic", 1, 10)])
    and usage("unaffected") is None
)
assert not debit([charge("atomic", 0, 5)]) and usage("atomic") == (10, 0)
print("atomic concurrent admission, no partial multi-meter charges, lower limits: PASS")
old = charge("old-window", 1, 1, "stable-meter")
old["window"]["end"] = 1
payload = json.dumps([old])
with sqlite3.connect(db) as c:
    c.execute(reserve[0], (payload,))
    c.execute(reserve[1], ("attempt", payload))
    c.execute(reserve[2], ("attempt", payload))
    c.execute("UPDATE resource_attempts SET state='uncertain' WHERE id='attempt'")
assert usage("old-window") == (0, 1)
assert not debit([charge("new-window", 1, 1, "stable-meter")])
with sqlite3.connect(db) as c:
    c.execute("UPDATE resource_attempts SET state='released' WHERE id='attempt'")
assert debit([charge("new-window", 1, 1, "stable-meter")]) and usage("old-window") == (
    0,
    0,
)
print("uncertain reservations survive window rollover and release exactly once: PASS")
with sqlite3.connect(db) as c:
    c.execute(reserve[0], (payload,))
    c.execute(reserve[1], ("consume-once", payload))
    c.execute(reserve[2], ("consume-once", payload))
    c.execute("UPDATE resource_attempts SET state='consumed' WHERE id='consume-once'")
    c.execute("UPDATE resource_attempts SET state='consumed' WHERE id='consume-once'")
assert usage("old-window") == (1, 0)
gauge = charge("capacity", 1000, 400000000)
gauge["mode"] = "gauge"
assert debit([gauge], 0)
gauge["amount"] = 500
assert debit([gauge], 0)
assert usage("capacity") == (500, 0)
print("settlement idempotence and observed capacity replacement: PASS")
fixture.cleanup()
print("QUOTA SQL PASS")
