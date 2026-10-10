#!/usr/bin/env python3
"""Real CLI archive/sync acceptance; run inside scripts/run-workerd.py.

Build brains-server and brains-cli first. This owns one disposable native
server; the outer harness owns the empty Worker and its isolated R2/D1 state.
"""

import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.request


def exercise(root, local_url, local_token):
    repo = Path(__file__).resolve().parent.parent
    config = root / "config.json"
    config.write_text(json.dumps({"active": "local", "profiles": {
        "local": {"url": local_url, "token": local_token},
        "cloud": {"url": os.environ["BRAINS_BASE_URL"],
                  "token": os.environ["BRAINS_E2E_TOKEN"]},
    }}))
    config.chmod(0o600)
    env = {key: value for key, value in os.environ.items()
           if key not in ("BRAINS_URL", "BRAINS_TOKEN")}
    env["BRAIN_CONFIG"] = str(config)

    def cli(profile, *args, data=None, expected=0):
        result = subprocess.run(
            [str(repo / "target/debug/bsx"), "--profile", profile, *map(str, args)],
            input=data, capture_output=True, env=env, timeout=120,
        )
        assert result.returncode == expected, (
            args, result.returncode, result.stdout.decode(errors="replace"),
            result.stderr.decode(errors="replace"),
        )
        return result.stdout

    def stat(profile, path):
        return json.loads(cli(profile, "stat", path, "--json"))

    def put(profile, path, data):
        cli(profile, "put", path, "--if-revision", stat(profile, path)["revision"], data=data)

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


def main():
    repo = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix="brains-cli-workflows-") as directory:
        root = Path(directory)
        token = secrets.token_urlsafe(32)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        with (root / "server.log").open("w+") as log:
            server = subprocess.Popen([
                str(repo / "target/debug/brains-server"), "--data-dir", str(root / "data"),
                "--bind", "127.0.0.1", "--port", str(port), "--token", token,
                "--vault", "cli-proof", "--no-seed",
            ], stdout=log, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 30
                while True:
                    assert server.poll() is None, "native server exited"
                    try:
                        request = urllib.request.Request(base + "/api/v1/session",
                            headers={"Authorization": "Bearer " + token})
                        with urllib.request.urlopen(request, timeout=1):
                            break
                    except OSError:
                        if time.monotonic() >= deadline:
                            raise TimeoutError("native server did not become ready") from None
                        time.sleep(0.1)
                exercise(root, base, token)
            finally:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    main()
