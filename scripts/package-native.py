#!/usr/bin/env python3
"""Package the already-built CLI/server pair and verify their source identity."""

import argparse
import hashlib
from pathlib import Path
import shutil
import subprocess
import tarfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True, choices=(
        "macos-arm64", "macos-x64", "linux-x64", "linux-arm64", "windows-x64"))
    parser.add_argument("--sha", required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("."))
    parser.add_argument("--readme", type=Path, default=Path("docs/native-release.md"))
    args = parser.parse_args()
    windows = args.target.startswith("windows-")
    suffix = ".exe" if windows else ""
    prefix = f"brains-{args.version}-{args.target}"
    directory = args.output_dir / prefix
    directory.mkdir(parents=True, exist_ok=False)
    expected = f"{args.version} (git {args.sha[:12]})"
    for name in ("bsx", "brains-server"):
        binary = (args.bin_dir / f"{name}{suffix}").resolve()
        actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
        if actual != f"{name} {expected}":
            raise SystemExit(f"Compiled identity mismatch: {actual}; expected {name} {expected}")
        shutil.copy2(binary, directory / binary.name)
    shutil.copyfile(args.readme, directory / "README.md")
    (directory / "VERSION").write_text(f"bsx {expected}\n", encoding="utf-8")
    archive = args.output_dir / f"{prefix}.{'zip' if windows else 'tar.gz'}"
    if windows:
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for file in sorted(directory.iterdir()):
                output.write(file, f"{prefix}/{file.name}")
    else:
        with tarfile.open(archive, "w:gz") as output:
            output.add(directory, arcname=prefix)
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="utf-8")
    print(f"Packaged {archive.name}; verified bsx and brains-server at {args.sha}")


if __name__ == "__main__":
    main()
