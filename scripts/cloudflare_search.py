"""Drive one explicit, bounded semantic refresh using the Brains admin API."""

import json
import time
import urllib.error
import urllib.request


def refresh(base, token, opener, *, max_pages=64, max_steps=120, timeout=900):
    if min(max_pages, max_steps, timeout) <= 0:
        raise ValueError("Refresh page, step and time bounds must be positive")
    deadline = time.monotonic() + timeout

    def post(path, body):
        if time.monotonic() >= deadline:
            raise ValueError("Semantic refresh time bound reached; queue remains available for explicit reconciliation")
        request = urllib.request.Request(
            base + "/api/v1" + path, method="POST", data=json.dumps(body).encode(),
            headers={"Authorization": "Bearer " + token, "Content-Type": "application/json",
                     "User-Agent": "brains-cloudflare-check"},
        )
        try:
            with opener.open(request, timeout=min(45, max(1, deadline-time.monotonic()))) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise ValueError(f"Semantic refresh {path}: HTTP {error.code}; inspect quota policy, authentication and queue state") from None

    cursor = None
    for _ in range(max_pages):
        body = {"limit": 2}
        if cursor:
            body["cursor"] = cursor
        page = post("/maintenance/reindex", body)
        if page["failed"]:
            raise ValueError("Source reindex reported failures; fix them before resuming semantic work")
        cursor = page.get("next_cursor")
        if not cursor:
            break
    else:
        raise ValueError("Source reindex page bound reached; not all sources were queued")

    for _ in range(max_steps):
        result = post("/maintenance/semantic/reconcile", {"limit": 2})
        states = result["states"]
        print(json.dumps({"processed": result["processed"], "states": states}), flush=True)
        if any(states.get(state, 0) for state in ("failed", "blocked", "delete_failed")):
            raise ValueError("Semantic work is paused; inspect provider errors or allowances before explicit retry")
        if not any(states.get(state, 0) for state in ("pending", "queued", "deleting")):
            return states
        if result["processed"] == 0:
            time.sleep(min(3, max(0, deadline-time.monotonic())))
    raise ValueError("Semantic reconciliation step bound reached; no automatic continuation was scheduled")
