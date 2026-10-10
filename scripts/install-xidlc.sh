#!/usr/bin/env bash
# Install the released compiler matching the runtime in Cargo.toml.
set -euo pipefail
cd "$(dirname "$0")/.."
compiler_version=$(python3 - <<'PY'
import tomllib
with open("Cargo.toml", "rb") as manifest:
    runtime = tomllib.load(manifest)["workspace"]["dependencies"]["xidl-rust-axum"]
print(runtime["version"].removeprefix("="))
PY
)
# Registry sources are not a Git checkout. Do not derive generated headers
# from an unrelated parent repository or the caller's build environment.
XIDLC_VERSION="$compiler_version" XIDLC_GIT_HASH=unknown "${CARGO:-cargo}" install --locked \
  --version "=$compiler_version" xidlc --features cli,fmt \
  --root "${1:?pass the installation directory}"
