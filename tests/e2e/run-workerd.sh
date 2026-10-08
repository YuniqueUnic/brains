#!/bin/sh
# Deterministic workerd e2e (BRAINS_BASE_URL against wrangler dev).
#
# Root cause this script exists: `wrangler dev` snapshots the asset manifest
# at startup and does not reliably re-index a rebuilt ../web/dist. A running
# instance then serves stale asset names; requests for the NEW hashed bundle
# fall through to the SPA fallback and return text/html — the browser rejects
# them with "Expected a JavaScript-or-Wasm module script but the server
# responded with a MIME type of text/html". Restarting wrangler after every
# dist rebuild guarantees the manifest matches the bundle under test.
#
# Usage: tests/e2e/run-workerd.sh [extra playwright args...]
set -e

PORT="${BRAINS_WORKERD_PORT:-8788}"
TOKEN="${BRAINS_E2E_TOKEN:-brains-e2e-root-token-0000000000000000}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LOG="${TMPDIR:-/tmp}/brains-wrangler-e2e.log"

# The local token registry must contain the e2e token's SHA-256 (id: e2e).
HASH=$(printf '%s' "$TOKEN" | shasum -a 256 | cut -d' ' -f1)

cd "$ROOT/apps/worker"

# Free the port and stop any wrangler instance serving a stale manifest.
lsof -ti "tcp:$PORT" 2>/dev/null | xargs kill 2>/dev/null || true
pkill -f "wrangler dev --port $PORT" 2>/dev/null || true
sleep 1

# First-run convenience: append the e2e entry to .dev.vars once (the file
# holds sha256 hashes only, never the token itself).
if [ -f .dev.vars ] && ! grep -q "\"e2e\"" .dev.vars; then
  python3 - "$TOKEN" "$HASH" <<'EOF'
import hashlib, json, re, sys
token, e2e_hash = sys.argv[1], sys.argv[2]
path = ".dev.vars"
lines = open(path).read().splitlines()
for i, line in enumerate(lines):
    if line.startswith("TOKEN_REGISTRY_JSON="):
        registry = json.loads(line[len("TOKEN_REGISTRY_JSON="):])
        if not any(t.get("id") == "e2e" for t in registry["tokens"]):
            registry["tokens"].append({"id": "e2e", "name": "e2e",
                                       "token_sha256": e2e_hash,
                                       "scopes": ["read:/", "write:/", "admin:/"],
                                       "enabled": True})
        lines[i] = "TOKEN_REGISTRY_JSON=" + json.dumps(registry, separators=(",", ":"))
open(path, "w").write("\n".join(lines) + "\n")
EOF
fi

echo "starting wrangler dev on :$PORT (log: $LOG)"
npx wrangler dev --port "$PORT" > "$LOG" 2>&1 &
WRANGLER_PID=$!
for _ in $(seq 1 60); do
  CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/api/v1/capabilities" 2>/dev/null || true)
  [ "$CODE" = "401" ] && break
  sleep 1
done
if [ "$CODE" != "401" ]; then
  echo "wrangler dev failed to become ready; log tail:" >&2
  tail -20 "$LOG" >&2
  exit 1
fi

cd "$ROOT/tests/e2e"
BRAINS_BASE_URL="http://127.0.0.1:$PORT" pnpm exec playwright test "$@"
STATUS=$?
echo "wrangler dev left running on :$PORT (pid $WRANGLER_PID, log: $LOG)"
exit $STATUS
