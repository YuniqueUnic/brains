#!/bin/bash
# Regenerate every committed API artifact from api/brains.idl.
# Requires xidlc 0.95.0: cargo install xidlc --locked --version 0.95.0 --features cli,fmt
set -euo pipefail
cd "$(dirname "$0")/.."
XIDLC="${XIDLC:-$HOME/.cargo/bin/xidlc}"
command -v "$XIDLC" >/dev/null || XIDLC=xidlc
if [ "$("$XIDLC" --version)" != "xidlc 0.95.0" ]; then
  echo 'Use xidlc 0.95.0 (set XIDLC to its executable); codegen is version-pinned.' >&2
  exit 1
fi

rm -rf sdk/openapi sdk/typescript
mkdir -p sdk/openapi sdk/typescript sdk/rust/src

"$XIDLC" gen --out-dir sdk/openapi openapi api/brains.idl
mv sdk/openapi/openapi_brains.json sdk/openapi/brains-openapi.json

"$XIDLC" gen --out-dir sdk/typescript typescript-rest api/brains.idl

# xidlc 0.95 gaps: `rust-axum --client` omits named IDL structs, and the
# `rust` target panics on interfaces — so generate types (from an interface-
# stripped copy) and the client separately, then merge into one module.
python3 scripts/splice-rust-types.py
"$XIDLC" gen --out-dir /tmp/brains-sdk-types rust /tmp/brains-types.idl
"$XIDLC" gen --out-dir /tmp/brains-sdk-rs rust-axum --client api/brains.idl
python3 scripts/fix-sdk-wire.py
python3 scripts/merge-rust-sdk.py
rustfmt --edition 2024 sdk/rust/src/brains_generated.rs

echo "sdk/ regenerated from api/brains.idl"
