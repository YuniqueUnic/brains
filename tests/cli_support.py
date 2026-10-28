"""Isolated process fixtures for real bsx business acceptance."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import time
import urllib.request

REPO = Path(__file__).resolve().parent.parent
EXE_SUFFIX = ".exe" if os.name == "nt" else ""
CLI_BIN = Path(os.environ.get("BRAINS_CLI_BIN", REPO / f"target/debug/bsx{EXE_SUFFIX}")).resolve()
SERVER_BIN = Path(os.environ.get("BRAINS_SERVER_BIN", REPO / f"target/debug/brains-server{EXE_SUFFIX}")).resolve()


class Cli:
    def __init__(self, root, binary=None):
        self.binary = binary or CLI_BIN
        self.config = root / "config.json"
        self.env = {key: value for key, value in os.environ.items()
                    if key not in ("BRAINS_URL", "BRAINS_TOKEN")}
        self.env["BRAIN_CONFIG"] = str(self.config)

    def __call__(self, profile, *args, data=None, expected=0):
        result = subprocess.run(
            [str(self.binary), "--profile", profile, *map(str, args)],
            input=data, capture_output=True, env=self.env, timeout=120,
        )
        assert result.returncode == expected, (
            args, result.returncode, result.stdout.decode(errors="replace"),
            result.stderr.decode(errors="replace"),
        )
        return result.stdout

    def stat(self, profile, path):
        return json.loads(self(profile, "stat", path, "--json"))

    def put(self, profile, path, data):
        self(profile, "put", path, "--if-revision",
             self.stat(profile, path)["revision"], data=data)

    def remote(self, name, url, token):
        self(name, "profile", "add", name, "--url", url)
        self(name, "login", "--stdin-token", data=token.encode())


def exercise_repeated_relations(cli, profile):
    """A normal note may repeat a link or tag; updates must still be searchable."""
    path = "relations/source.md"
    for tag, destination in (("first", "before.md"), ("second", "after.md")):
        body = (f"---\ntags: [{tag}, {tag}, shared]\n---\n# {tag}\n"
                f"[One]({destination})\n[Again](./{destination}#section)\n").encode()
        if tag == "first":
            cli(profile, "put", path, "--create", data=body)
        else:
            cli.put(profile, path, body)
        revision = cli.stat(profile, path)["revision"]
        indexed = [json.loads(line) for line in
                   cli(profile, "ls", "relations/", "--source", "index", "--tag", tag, "--tag", "shared", "--json").splitlines()]
        assert len(indexed) == 1 and indexed[0]["revision"] == revision
        for mode in ("literal", "fts"):
            hits = [json.loads(line) for line in
                    cli(profile, "search", tag, "--prefix", "relations/", "--tag", tag, "--tag", "shared",
                        "--mode", mode, "--json").splitlines()]
            assert len(hits) == 1 and hits[0]["indexed_revision"] == revision
        links = [json.loads(line) for line in cli(profile, "links", path, "--json").splitlines()]
        assert len(links) == 1 and links[0]["target"] == f"relations/{destination}"
        assert links[0]["source_revision"] == revision
        backlinks = [json.loads(line) for line in
                     cli(profile, "links", f"relations/{destination}", "--backlinks", "--json").splitlines()]
        assert len(backlinks) == 1 and backlinks[0]["source_revision"] == revision
    assert cli(profile, "links", "relations/before.md", "--backlinks", "--json") == b""
    assert cli(profile, "ls", "relations/", "--source", "index", "--tag", "first", "--json") == b""
    cli(profile, "reindex", "--all-pages")
    assert len(cli(profile, "links", path, "--json").splitlines()) == 1


def exercise_path_subtrees(cli, profile):
    """Store, literal, FTS and index listing agree on UTF-8 segment boundaries."""
    for prefix in ("scoped", "scoped.md", "docs/ÿ", "资料/缓存"):
        expected = [prefix, prefix + "/a.md", prefix + "/b.md"]
        for path in expected + [prefix + "-private.md", prefix + "2/private.md"]:
            cli(profile, "put", path, "--create", data=b"---\ntags: [boundary]\n---\nneedle\n")
        for requested in (prefix, prefix + "/"):
            for source in ("store", "index"):
                rows = [json.loads(line) for line in cli(profile, "ls", requested,
                        "--source", source, "--json").splitlines()]
                assert [row["path"] for row in rows] == expected
            for mode in ("literal", "fts"):
                rows = [json.loads(line) for line in cli(profile, "search", "needle",
                        "--prefix", requested, "--mode", mode,
                        "--json").splitlines()]
                assert sorted(row["path"] for row in rows) == [p for p in expected if p.endswith(".md")]


@contextmanager
def native_server(root, data_dir, vault, binary=None, token=None, register_token=True):
    token = token or secrets.token_urlsafe(32)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    with (root / f"server-{port}.log").open("w+") as log:
        env = {key: value for key, value in os.environ.items()
               if key not in ("BRAINS_TOKEN", "BRAINS_TOKEN_FILE")}
        if register_token:
            env["BRAINS_TOKEN"] = token
        server = subprocess.Popen([
            str(binary or SERVER_BIN), "--data-dir", str(data_dir),
            "--bind", "127.0.0.1", "--port", str(port), "--vault", vault, "--no-seed",
        ], env=env, stdout=log, stderr=subprocess.STDOUT,
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0)
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
            yield base, token
        finally:
            if server.poll() is None:
                if os.name == "nt":
                    server.send_signal(signal.CTRL_BREAK_EVENT)
                else:
                    server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
