#!/usr/bin/env python3
"""Exercise the actual operator calibration SQL against isolated SQLite."""

import contextlib
import datetime as dt
import io
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import urllib.error

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
import cloudflare_usage as usage

now = dt.datetime.now(dt.timezone.utc)
end = int((now.replace(hour=0, minute=0, second=0, microsecond=0) + dt.timedelta(days=1)).timestamp())
env = {"CLOUDFLARE_ACCOUNT_ID": "test", "BRAINS_D1_DATABASE_ID": "db"}
connection = sqlite3.connect(":memory:")
connection.row_factory = sqlite3.Row
connection.executescript((Path(__file__).resolve().parent.parent / "migrations/d1/0003_resources_semantic.sql").read_text())


def reset():
    connection.execute("DELETE FROM resource_usage")
    for key, rule, amount, reserved, reset_at in [
        ("read", "d1-reads", 1000, 7, end), ("write", "d1-writes", 500, 3, end),
        ("storage", "d1-database-storage", 900, 0, None),
    ]:
        connection.execute("INSERT INTO resource_usage(key,meter,rule,unit,evidence,limit_value,consumed,reserved,reset_at) VALUES(?,?,?,'row','bounded',100000,?,?,?)",
                           (key, key, rule, amount, reserved, reset_at))
    connection.commit()


class Api:
    account = "test"
    race = False
    lose_response = False

    def __init__(self, _env):
        pass

    def request(self, _method, path, body=None):
        if path == "/graphql":
            if self.race:
                connection.execute("UPDATE resource_usage SET consumed=consumed+1 WHERE key='read'")
            return {"viewer": {"accounts": [{"d1AnalyticsAdaptiveGroups": [{"sum": {"rowsRead": 100, "rowsWritten": 50}}]}]}}
        if path.startswith("/d1/database?"):
            return [{"uuid": "db", "file_size": 1000}, {"uuid": "other", "file_size": 200}]
        result = connection.execute(body["sql"], body["params"])
        rows = [dict(row) for row in result.fetchall()]
        connection.commit()
        if self.lose_response and "RETURNING key,rule" in body["sql"]:
            raise urllib.error.URLError("simulated lost response after commit")
        return [{"success": True, "results": rows}]


usage.CloudflareApi = Api
with tempfile.TemporaryDirectory(prefix="brains-calibration-") as directory:
    root = Path(directory)
    reset()
    receipt = root / "success.json"
    with contextlib.redirect_stdout(io.StringIO()):
        usage.synchronize(env, apply=True, receipt=receipt)
    report = json.loads(receipt.read_text())
    rows = {row["key"]: dict(row) for row in connection.execute("SELECT * FROM resource_usage")}
    assert report["state"] == "applied" and report["observation"]["databases"] == 2
    assert rows["read"]["consumed"] == 164 and rows["write"]["consumed"] == 82
    assert rows["read"]["reserved"] == 7 and rows["write"]["reserved"] == 3
    assert rows["read"]["evidence"] == "estimated" and rows["storage"]["consumed"] == 1000
    assert receipt.stat().st_mode & 0o777 == 0o600
    snapshot = root / "snapshot.json"
    snapshot.write_text(json.dumps(report["observation"]))
    connection.execute("UPDATE resource_usage SET consumed=consumed+1 WHERE key='read'")
    try:
        usage.synchronize(env, apply=True, snapshot_file=snapshot, receipt=root / "replay.json")
        raise AssertionError("replayed snapshot overwrote newer consumption")
    except ValueError as error:
        assert "changed during calibration" in str(error)
    assert connection.execute("SELECT consumed FROM resource_usage WHERE key='read'").fetchone()[0] == 165
    reset()
    Api.race = True
    try:
        usage.synchronize(env, apply=True, receipt=root / "race.json")
        raise AssertionError("consumption during observation was lost")
    except ValueError as error:
        assert "changed during calibration" in str(error)
    finally:
        Api.race = False
    assert connection.execute("SELECT consumed FROM resource_usage WHERE key='write'").fetchone()[0] == 500
    mixed = report["observation"]
    mixed["baseline"][0]["reset_at"] = end - 86400
    snapshot.write_text(json.dumps(mixed))
    try:
        usage.synchronize(env, apply=True, snapshot_file=snapshot, receipt=root / "mixed.json")
        raise AssertionError("mixed-day calibration was accepted")
    except ValueError as error:
        assert "UTC-day boundary" in str(error)
    reset()
    Api.lose_response = True
    try:
        usage.synchronize(env, apply=True, receipt=root / "uncertain.json")
        raise AssertionError("lost response appeared successful")
    except urllib.error.URLError:
        assert json.loads((root / "uncertain.json").read_text())["state"] == "unconfirmed"
    finally:
        Api.lose_response = False
print("D1 calibration: all-or-nothing CAS, preserved reservations, replay/concurrency/day guards, private and uncertain receipts: PASS")
