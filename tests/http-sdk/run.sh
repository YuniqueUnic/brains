#!/usr/bin/env bash
# Real HTTP acceptance using released runtimes and disposable brains data.
# BRAINS_BASE_URL + BRAINS_E2E_TOKEN select a caller-owned server (e.g. workerd).
set -euo pipefail
cd "$(dirname "$0")/../.."
repo_dir=$PWD
probe_dir=$(mktemp -d)
server_pid=''
cleanup() {
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  rm -rf "$probe_dir"
}
trap cleanup EXIT
cp tests/http-sdk/{business.ts,package.json,package-lock.json,tsconfig.json} "$probe_dir/"
cp -R sdk/typescript "$probe_dir/sdk"
export BRAINS_TEST_TOKEN BRAINS_TEST_ORIGIN
if [[ -n "${BRAINS_BASE_URL:-}" ]]; then
  BRAINS_TEST_ORIGIN=$BRAINS_BASE_URL
  BRAINS_TEST_TOKEN=${BRAINS_E2E_TOKEN:?set BRAINS_E2E_TOKEN for the selected server}
else
  if [[ -z "${BRAINS_SERVER_BIN:-}" ]]; then
    "${CARGO:-cargo}" build -p brains-server
    BRAINS_SERVER_BIN="$repo_dir/target/debug/brains-server"
  fi
  BRAINS_TEST_TOKEN=$(node -p "require('node:crypto').randomBytes(24).toString('hex')")
  port=${BRAINS_SDK_TEST_PORT:-$(node -e 'const s=require("node:net").createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')}
  BRAINS_TEST_ORIGIN="http://127.0.0.1:$port"
  "$BRAINS_SERVER_BIN" --data-dir "$probe_dir/data" --bind 127.0.0.1 \
    --port "$port" --token "$BRAINS_TEST_TOKEN" \
    --origin "$BRAINS_TEST_ORIGIN" > "$probe_dir/server.log" 2>&1 &
  server_pid=$!
  ready=false
  for attempt in {1..100}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then cat "$probe_dir/server.log"; exit 1; fi
    if curl --silent --fail --header "Authorization: Bearer $BRAINS_TEST_TOKEN" \
      "$BRAINS_TEST_ORIGIN/api/v1/session" >/dev/null; then ready=true; break; fi
    sleep 0.1
  done
  if [[ "$ready" != true ]]; then cat "$probe_dir/server.log"; exit 1; fi
fi
cd "$probe_dir"
npm ci --ignore-scripts --no-audit --no-fund
npx --no-install tsc --noEmit -p .
npx --no-install tsx business.ts || { if [[ -f server.log ]]; then cat server.log; fi; exit 1; }
