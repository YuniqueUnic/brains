#!/usr/bin/env bash
# Install the compiler pinned in Cargo.toml into the supplied directory.
set -euo pipefail
cd "$(dirname "$0")/.."
read -r compiler_git compiler_rev < <(python3 - <<'PY'
import tomllib
with open("Cargo.toml", "rb") as manifest:
    compiler = tomllib.load(manifest)["workspace"]["metadata"]["xidl"]["compiler"]
print(compiler["git"], compiler["rev"])
PY
)
XIDLC_GIT_HASH="$compiler_rev" "${CARGO:-cargo}" install --locked \
  --git "$compiler_git" --rev "$compiler_rev" xidlc --features cli,fmt \
  --root "${1:?pass the installation directory}"
