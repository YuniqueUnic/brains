#!/usr/bin/env python3
"""Real CLI archive/sync acceptance; run inside scripts/run-workerd.py.

Build brains-server and brains-cli first. This owns one disposable native
server; the outer harness owns the empty Worker and its isolated R2/D1 state.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

from cli_support import Cli, exercise_repeated_relations, native_server


def exercise(root, source_mode, local_url=None, local_token=None):
    cli = Cli(root)
    if source_mode == "local":
        cli("local", "init", root / "data", "--vault", "cli-proof")
    else:
        cli.remote("local", local_url, local_token)
    cli.remote("cloud", os.environ["BRAINS_BASE_URL"], os.environ["BRAINS_E2E_TOKEN"])
    stat, put = cli.stat, cli.put

    assert cli("cloud", "ls", "--json", "--deleted") == b"", "Worker must be empty"
    source = root / "input"
    fixtures = {
        "notes/topic.md": b"alpha\nbeta\ngamma\n",
        "bin/run.sh": b"#!/bin/sh\necho archive\n",
        "assets/raw.bin": bytes([0, 255, 1, 128]),
        "trash/deleted.bin": bytes([255, 0, 2]),
    }
    for path, data in fixtures.items():
        target = source / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (source / "bin/run.sh").chmod(0o755)
    cli("local", "import", source)
    original = stat("local", "notes/topic.md")["revision"]
    put("local", "notes/topic.md", b"changed\nbeta\ngamma\n")
    cli("local", "restore", "notes/topic.md", "--revision", original,
        "--if-revision", stat("local", "notes/topic.md")["revision"])
    cli("local", "rm", "trash/deleted.bin", "--if-revision",
        stat("local", "trash/deleted.bin")["revision"])

    archive = root / "archive"
    exported = json.loads(cli("local", "export", archive, "--format", "archive"))
    assert (exported["paths"], exported["revisions"], exported["blobs"]) == (4, 7, 5)
    assert exported["history_verified"]
    cli("cloud", "import", archive, "--format", "archive", "--dry-run")
    assert cli("cloud", "ls", "--json", "--deleted") == b""
    for published in (7, 0):
        imported = json.loads(cli("cloud", "import", archive, "--format", "archive"))
        assert imported["published"] == published
        assert imported["history_verified"] and imported["index_verified"]
    for path in ("notes/topic.md", "bin/run.sh", "assets/raw.bin"):
        assert cli("cloud", "cat", path) == fixtures[path]
    working = root / "working-cloud"
    cli("cloud", "export", working)
    assert (working / "bin/run.sh").stat().st_mode & 0o111
    for path in fixtures:
        left = [json.loads(line) for line in cli("local", "history", path, "--json").splitlines()]
        right = [json.loads(line) for line in cli("cloud", "history", path, "--json").splitlines()]
        assert left == right, path

    roundtrip = root / "roundtrip"
    cli("cloud", "export", roundtrip, "--format", "archive")

    def normalized_manifest(directory):
        value = json.loads((directory / "manifest.json").read_text())
        vault = value.pop("vault")
        for file in value["files"]:
            for revision in file["revisions"]:
                content = revision["content"]
                if content is not None:
                    key = content["key"]
                    blob = directory / "blobs" / hashlib.sha256(key.encode()).hexdigest()
                    assert hashlib.sha256(blob.read_bytes()).hexdigest() == content["sha256"]
                    content["key"] = key.removeprefix(f"vaults/{vault}/")
        return value

    assert normalized_manifest(archive) == normalized_manifest(roundtrip)
    script = next(file for file in normalized_manifest(archive)["files"]
                  if file["head"]["path"] == "bin/run.sh")
    assert script["revisions"][0]["content"]["executable"] is True
    print("archive: 4 paths / 7 revisions / 5 blobs; retry, history, modes and hashes passed")

    sync_args = ("sync", "--remote-profile", "cloud")
    cli("local", *sync_args, "--dry-run")
    assert not (root / "sync").exists(), "dry-run must not write sync state"
    cli("local", *sync_args)
    put("local", "notes/topic.md", b"ALPHA\nbeta\ngamma\n")
    put("cloud", "notes/topic.md", b"alpha\nbeta\nGAMMA\n")
    cli("local", *sync_args)
    for profile in ("local", "cloud"):
        assert cli(profile, "cat", "notes/topic.md") == b"ALPHA\nbeta\nGAMMA\n"

    put("local", "assets/raw.bin", b"\x00\xfflocal")
    put("cloud", "assets/raw.bin", b"\x00\xffremote")
    conflict = cli("local", *sync_args, expected=5)
    assert b"conflict assets/raw.bin" in conflict
    assert cli("local", "cat", "assets/raw.bin") == b"\x00\xfflocal"
    assert cli("cloud", "cat", "assets/raw.bin") == b"\x00\xffremote"
    cli("local", *sync_args, "resolve", "assets/raw.bin", "--keep", "remote")
    assert cli("local", "cat", "assets/raw.bin") == b"\x00\xffremote"
    cli("local", "rm", "bin/run.sh", "--if-revision", stat("local", "bin/run.sh")["revision"])
    cli("local", *sync_args)
    remote_files = [json.loads(line) for line in cli("cloud", "ls", "--json", "--deleted").splitlines()]
    assert next(item for item in remote_files if item["path"] == "bin/run.sh")["state"] == "trashed"
    cli("local", *sync_args, "verify")
    print("sync: dry-run, disjoint text merge, binary conflict/resolution and delete passed")
    for profile in ("local", "cloud"):
        exercise_repeated_relations(cli, profile)
    print("index: repeated links/tags update, search, backlinks and reindex passed on both endpoints")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-mode", choices=("local", "remote"), default="remote")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="brains-cli-workflows-") as directory:
        root = Path(directory)
        if args.source_mode == "local":
            exercise(root, "local")
        else:
            with native_server(root, root / "data", "cli-proof") as (base, token):
                exercise(root, "remote", base, token)
    print(f"{args.source_mode} source ↔ Worker: all workflows passed")


if __name__ == "__main__":
    main()
