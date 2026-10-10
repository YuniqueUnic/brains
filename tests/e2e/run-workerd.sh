#!/bin/sh
# Build web/worker first; each run owns its temporary secrets, state and server.
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
exec python3 "$ROOT/scripts/run-workerd.py" -- pnpm --dir tests/e2e exec playwright test "$@"
