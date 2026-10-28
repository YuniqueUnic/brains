#!/usr/bin/env python3
"""Enforce slash-separated literal segments in declared and generated API routes."""

import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent


def invalid(path):
    if (
        not path.startswith("/")
        or "//" in path
        or (len(path) > 1 and path.endswith("/"))
    ):
        return True
    return any(
        not re.fullmatch(r"[a-z0-9]+|\{[a-zA-Z_][a-zA-Z0-9_]*\}", segment)
        for segment in path.split("/")[1:]
        if segment
    )


def main():
    declared = re.findall(
        r'\bpath\s*=\s*"([^"]+)"', (ROOT / "api/brains.idl").read_text()
    )
    generated = json.loads((ROOT / "sdk/openapi/brains-openapi.json").read_text())[
        "paths"
    ]
    violations = sorted({p for p in [*declared, *generated] if invalid(p)})
    if violations:
        for path in violations:
            print(
                f"Invalid API path {path}: split literal words with '/', using lowercase letters and digits.",
                file=sys.stderr,
            )
        return 1
    print(
        f"API paths: {len(set(declared))} declared routes and {len(generated)} OpenAPI paths follow segment rules."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
