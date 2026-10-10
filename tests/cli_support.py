"""Isolated process fixtures for real bsx business acceptance."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time
import urllib.request

REPO = Path(__file__).resolve().parent.parent


class Cli:
    def __init__(self, root):
        self.config = root / "config.json"
        self.env = {key: value for key, value in os.environ.items()
                    if key not in ("BRAINS_URL", "BRAINS_TOKEN")}
        self.env["BRAIN_CONFIG"] = str(self.config)

    def __call__(self, profile, *args, data=None, expected=0):
        result = subprocess.run(
            [str(REPO / "target/debug/bsx"), "--profile", profile, *map(str, args)],
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


@contextmanager
def native_server(root, data_dir, vault):
    token = secrets.token_urlsafe(32)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    with (root / f"server-{port}.log").open("w+") as log:
        env = dict(os.environ, BRAINS_TOKEN=token)
        server = subprocess.Popen([
            str(REPO / "target/debug/brains-server"), "--data-dir", str(data_dir),
            "--bind", "127.0.0.1", "--port", str(port), "--vault", vault, "--no-seed",
        ], env=env, stdout=log, stderr=subprocess.STDOUT)
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
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
