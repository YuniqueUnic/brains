#!/bin/bash
# Regenerate every committed API artifact from api/brains.idl.
# Requires xidlc 0.96+: cargo install xidlc --locked --features cli,fmt
set -euo pipefail
cd "$(dirname "$0")/.."
XIDLC="${XIDLC:-$HOME/.cargo/bin/xidlc}"
command -v "$XIDLC" >/dev/null || XIDLC=xidlc

rm -rf sdk/openapi sdk/typescript
mkdir -p sdk/openapi sdk/typescript sdk/rust/src

"$XIDLC" gen --out-dir sdk/openapi openapi api/brains.idl
mv sdk/openapi/openapi_brains.json sdk/openapi/brains-openapi.json

"$XIDLC" gen --out-dir sdk/typescript typescript-rest api/brains.idl

# xidlc 0.96 emits module types in the rust-axum client (xidl#295 part 1),
# so one generation covers types + client. The @optional-header compile bug
# (xidl#295 part 2, still open) is patched by scripts/patch-rust-sdk.py.
"$XIDLC" gen --out-dir /tmp/brains-sdk-rs rust-axum --client api/brains.idl
python3 scripts/patch-rust-sdk.py

echo "sdk/ regenerated from api/brains.idl"
