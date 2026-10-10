#!/usr/bin/env python3
"""Check product versions without changing files or touching dependency versions."""

import json
from pathlib import Path
import subprocess
import tomllib


def main():
    root = Path(__file__).resolve().parent.parent
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    version = workspace["package"]["version"]
    versions = {
        "VERSION.txt": (root / "VERSION.txt").read_text().strip(),
        ".release-please-manifest.json": json.loads(
            (root / ".release-please-manifest.json").read_text()
        )["."],
    }
    lock = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    for member in workspace["members"]:
        package = tomllib.loads((root / member / "Cargo.toml").read_text())["package"]
        if package.get("version") != {"workspace": True}:
            raise SystemExit(f"{member} must inherit workspace.package.version")
        entries = [p for p in lock if p["name"] == package["name"] and "source" not in p]
        if len(entries) != 1:
            raise SystemExit(f"Cargo.lock must contain one local {package['name']}")
        versions[f"Cargo.lock:{package['name']}"] = entries[0]["version"]
    packages = subprocess.check_output(
        ["git", "ls-files", "*package.json", "package.json"], cwd=root, text=True
    ).splitlines()
    for path in packages:
        versions[path] = json.loads((root / path).read_text()).get("version")
    npm_lock = json.loads((root / "tests/http-sdk/package-lock.json").read_text())
    versions["http-sdk lock"] = npm_lock["version"]
    versions["http-sdk lock root"] = npm_lock["packages"][""]["version"]
    errors = [f"{path}: {actual!r}" for path, actual in versions.items() if actual != version]
    if errors:
        raise SystemExit(f"Expected Brains {version}:\n" + "\n".join(errors))
    print(f"Brains {version}: {len(versions)} version entries agree")


if __name__ == "__main__":
    main()
