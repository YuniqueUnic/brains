"""Observe account D1 usage and explicitly rebase conservative admission counters.

This is an operator action, not a background poller or an invoice guarantee.
Provider observations and the previous ledger are retained in a private receipt.
"""

import datetime as dt
import json
from pathlib import Path

from cloudflare_api import CloudflareApi


def snapshot(api, database, baseline):
    now = dt.datetime.now(dt.timezone.utc)
    day = now.date().isoformat()
    data = api.request("POST", "/graphql", {
        "query": """query($account:string!,$day:Date){viewer{accounts(filter:{accountTag:$account}){
          d1AnalyticsAdaptiveGroups(limit:1,filter:{date_geq:$day,date_leq:$day}){
            sum{rowsRead rowsWritten}dimensions{date}
          }
        }}}""",
        "variables": {"account": api.account, "day": day},
    })
    accounts = data["viewer"]["accounts"]
    if not accounts or not accounts[0]["d1AnalyticsAdaptiveGroups"]:
        raise ValueError("No account D1 Analytics observation is available for this UTC day")
    totals = accounts[0]["d1AnalyticsAdaptiveGroups"][0]["sum"]
    databases = api.request("GET", "/d1/database?per_page=100")
    selected = next(item for item in databases if item["uuid"] == database)
    return {
        "account": api.account, "database": database, "day": day,
        "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(), "baseline": baseline, "source": "Cloudflare account D1 Analytics and database inventory",
        "rows_read": totals["rowsRead"], "rows_written": totals["rowsWritten"],
        "databases": len(databases),
        "account_storage_bytes": sum(item["file_size"] for item in databases),
        "database_storage_bytes": selected["file_size"],
    }


def synchronize(env, *, apply=False, snapshot_file=None, receipt=None):
    api = CloudflareApi(env)
    database = env["BRAINS_D1_DATABASE_ID"]
    now = dt.datetime.now(dt.timezone.utc)
    end = int((now.replace(hour=0, minute=0, second=0, microsecond=0) + dt.timedelta(days=1)).timestamp())
    target = f"/d1/database/{database}/query"

    def query(sql, params=None):
        result = api.request("POST", target, {"sql": sql, "params": params or []})
        if not all(item.get("success") for item in result):
            raise ValueError("D1 usage calibration failed")
        return result[0]["results"]

    if snapshot_file:
        observation = json.loads(Path(snapshot_file).read_text())
    else:
        # Capture the ledger BEFORE asking Analytics. CAS must protect all app
        # consumption that occurs while the provider observation is obtained.
        baseline = query("SELECT * FROM resource_usage WHERE (rule IN ('d1-reads','d1-writes') AND reset_at=?1) OR rule='d1-database-storage'", [end])
        observation = snapshot(api, database, baseline)
    observed = dt.datetime.fromisoformat(observation["observed_at"])
    now = dt.datetime.now(dt.timezone.utc)
    if (observation["account"] != api.account or observation["database"] != database
        or observation["day"] != now.date().isoformat()
        or not 0 <= (now - observed).total_seconds() <= 300):
        raise ValueError("Use a fresh observation for the configured account, database and UTC day")
    before = observation.get("baseline")
    if not before:
        raise ValueError("An external observation must include its pre-observation D1 ledger baseline")
    current_end = int((now.replace(hour=0, minute=0, second=0, microsecond=0) + dt.timedelta(days=1)).timestamp())
    for row in before:
        if row["rule"] not in ("d1-reads", "d1-writes", "d1-database-storage"):
            raise ValueError("A D1 calibration baseline may contain only D1 admission and target storage meters")
        if row["rule"] in ("d1-reads", "d1-writes") and row["reset_at"] != current_end:
            raise ValueError("Provider observation and ledger baseline cross a UTC-day boundary; capture both again")
    # Retain a conservative allowance for the calibration's own small SQL batch.
    values = {"d1-reads": observation["rows_read"] + 64,
              "d1-writes": observation["rows_written"] + 32,
              "d1-database-storage": observation["database_storage_bytes"]}
    changes = []
    for row in before:
        amount = values[row["rule"]]
        if not isinstance(amount, (float, int)) or amount < 0:
            raise ValueError("Provider quantities must be non-negative numbers")
        changes.append({"key": row["key"], "expected": row["consumed"],
                        "reserved": row["reserved"], "consumed": amount,
                        "evidence": "exact" if row["rule"] == "d1-database-storage" else "estimated"})
    if not {"d1-reads", "d1-writes"}.issubset({row["rule"] for row in before}):
        raise ValueError("Current-day D1 admission meters are missing; first make an authenticated request")
    report = {"observation": observation, "before": before, "changes": changes, "state": "preview"}
    if apply:
        if not receipt:
            raise ValueError("--receipt is required when applying a provider calibration")
        path = Path(receipt).expanduser()
        path.parent.mkdir(parents=True, exist_ok=True)
        # Persist the original counters before issuing the conditional write.
        report["state"] = "unconfirmed"
        with path.open("x") as output:
            path.chmod(0o600)
            json.dump(report, output, indent=2)
        rows = query("""WITH admission AS MATERIALIZED (SELECT NOT EXISTS (
              SELECT 1 FROM json_each(?1) c LEFT JOIN resource_usage u ON u.key=json_extract(c.value,'$.key')
              WHERE u.key IS NULL OR u.consumed<>json_extract(c.value,'$.expected') OR u.reserved<>json_extract(c.value,'$.reserved')
            ) AS allowed)
            UPDATE resource_usage SET
            consumed=(SELECT json_extract(value,'$.consumed') FROM json_each(?1) WHERE json_extract(value,'$.key')=resource_usage.key),
            evidence=(SELECT json_extract(value,'$.evidence') FROM json_each(?1) WHERE json_extract(value,'$.key')=resource_usage.key)
            WHERE key IN (SELECT json_extract(value,'$.key') FROM json_each(?1))
            AND (SELECT allowed FROM admission)
            RETURNING key,rule,consumed,reserved,evidence""", [json.dumps(changes)])
        if len(rows) != len(changes):
            report["state"] = "not_applied"
            path.write_text(json.dumps(report, indent=2) + "\n")
            raise ValueError("D1 counters changed during calibration; no counters were rebased. Capture a fresh observation")
        report.update(state="applied", after=rows)
        path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
