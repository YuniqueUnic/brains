#!/usr/bin/env bash
# Real HTTP acceptance against disposable brains data and the selected xidl runtime.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${XIDL_SOURCE:?set XIDL_SOURCE to the tested xidl checkout}"
XIDL_SOURCE=$(cd "$XIDL_SOURCE" && pwd)
repo_dir=$PWD
probe_dir=$(mktemp -d)
server_pid=''
cleanup() {
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  rm -rf "$probe_dir"
}
trap cleanup EXIT
cp tests/http-sdk/{business.ts,package.json,tsconfig.json} "$probe_dir/"
cp -R sdk/typescript "$probe_dir/sdk"
for runtime in xidl-typescript-codec xidl-typescript-client; do
  pnpm --dir "$XIDL_SOURCE/typescript/$runtime" install --frozen-lockfile --ignore-scripts
  pnpm --dir "$XIDL_SOURCE/typescript/$runtime" build
done
cargo build -p brains-server
export BRAINS_TEST_TOKEN
BRAINS_TEST_TOKEN=$(node -p "require('node:crypto').randomBytes(24).toString('hex')")
export BRAINS_TEST_ORIGIN="http://127.0.0.1:${BRAINS_SDK_TEST_PORT:-45962}"
"$repo_dir/target/debug/brains-server" --data-dir "$probe_dir/data" --bind 127.0.0.1 \
  --port "${BRAINS_SDK_TEST_PORT:-45962}" --token "$BRAINS_TEST_TOKEN" \
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
cd "$probe_dir"
npm install --ignore-scripts --no-audit --no-fund \
  "$XIDL_SOURCE/typescript/xidl-typescript-client" "$XIDL_SOURCE/typescript/xidl-typescript-codec"
npx tsc --noEmit -p .
npx tsx business.ts || { cat "$probe_dir/server.log"; exit 1; }
