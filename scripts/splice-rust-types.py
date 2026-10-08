"""Assemble sdk/rust/src/brains_generated.rs from two xidlc outputs.

`xidlc gen rust-axum --client` (0.95) emits the client but NOT the named
IDL structs; `xidlc gen rust` emits the structs but panics once an interface
is present. Single source of truth stays api/brains.idl: this script strips
the interface blocks to a types-only IDL, generates both halves, and merges
them into one module. Drop the whole splice once upstream emits types.
"""
import re
from pathlib import Path

source = Path("api/brains.idl").read_text()
types_idl = re.sub(
    r"@(http_bearer|no_security)\ninterface \w+ \{.*?\n\};\n\n?",
    "",
    source,
    flags=re.S,
)
Path("/tmp/brains-types.idl").write_text(types_idl)
print("types-only idl written")
