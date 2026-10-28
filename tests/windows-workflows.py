#!/usr/bin/env python3
"""Exercise the packaged Windows CLI/server in isolated Unicode data paths."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

from cli_support import Cli, native_server


class WindowsWorkflows(unittest.TestCase):
    binary = None
    server = None

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="brains Windows 资料 ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.data = self.root / "知识库 data"
        self.cli = Cli(self.root, self.binary)
        self.cli("local", "init", self.data)

    def test_home_config_is_independent_of_cwd_and_unix_home(self):
        home = self.root / "Windows home"
        first = self.root / "first cwd"
        second = self.root / "second cwd"
        for directory in (home, first, second):
            directory.mkdir()
        env = {key: value for key, value in os.environ.items()
               if key not in ("HOME", "BRAIN_CONFIG", "BRAINS_URL", "BRAINS_TOKEN")}
        env["USERPROFILE"] = str(home)
        subprocess.run([str(self.binary), "--profile", "home", "init", str(home / "data")],
                       cwd=first, env=env, check=True, capture_output=True)
        subprocess.run([str(self.binary), "--profile", "home", "ls"],
                       cwd=second, env=env, check=True, capture_output=True)
        self.assertTrue((home / ".brains/config.json").is_file())
        self.assertFalse((first / ".brains").exists())
        self.assertFalse((second / ".brains").exists())

    def test_windows_names_fail_without_touching_existing_content(self):
        cli = self.cli
        cli("local", "put", "Folder/Note.md", "--create", data=b"original")
        outside = self.root / "outside.md"
        outside.write_bytes(b"outside sentinel")
        drive_path = str(outside).replace("\\", "/")
        for path in (drive_path, "C:drive.md", "ads.md:stream", "NUL.md", "COM1.md",
                     "COM¹.md", "LPT³.md", "trailing./x.md", "space /x.md", "tail.md.",
                     "folder/other.md", "Folder/note.md"):
            cli("local", "put", path, "--create", data=b"reject", expected=2)
        self.assertEqual(cli("local", "cat", "Folder/Note.md"), b"original")
        self.assertEqual(outside.read_bytes(), b"outside sentinel")
        rows = [json.loads(line) for line in cli("local", "ls", "--json").splitlines()]
        self.assertEqual([row["path"] for row in rows], ["Folder/Note.md"])

    def test_server_restart_preserves_auth_content_and_history(self):
        cli = self.cli
        cli("local", "put", "资料/note.md", "--create", data="缓存 first\n".encode())
        first = cli.stat("local", "资料/note.md")["revision"]
        with native_server(self.root, self.data, "default", self.server) as (url, token):
            cli.remote("server", url, token)
            cli("local", "ls", expected=6)
            cli.put("server", "资料/note.md", "缓存 second\n".encode())
            current = cli.stat("server", "资料/note.md")["revision"]
        auth = (self.data / "auth.json").read_bytes()
        signing_key = (self.data / "secret.key").read_bytes()
        with native_server(self.root, self.data, "default", self.server,
                           token=token, register_token=False) as (url, _):
            cli.remote("restarted", url, token)
            self.assertEqual(cli.stat("restarted", "资料/note.md")["revision"], current)
            self.assertEqual(cli("restarted", "show", "资料/note.md", "--revision", first),
                             "缓存 first\n".encode())
            hits = [json.loads(line) for line in
                    cli("restarted", "search", "second", "--mode", "fts", "--json").splitlines()]
            self.assertEqual(len(hits), 1)
            self.assertEqual(hits[0]["indexed_revision"], current)
        self.assertEqual((self.data / "auth.json").read_bytes(), auth)
        self.assertEqual((self.data / "secret.key").read_bytes(), signing_key)
        self.assertEqual(cli("local", "cat", "资料/note.md"), "缓存 second\n".encode())
        for log in self.root.glob("server-*.log"):
            self.assertIn("shutdown requested", log.read_text(encoding="utf-8"))

    def test_packaged_conversion_remains_available(self):
        original = b"<h1>Windows conversion</h1><p>Readable content.</p>"
        self.cli("local", "put", "document.html", "--create", data=original)
        self.cli("local", "convert", "document.html")
        output = self.cli("local", "cat", "document.html.md")
        self.assertIn(b"Windows conversion", output)
        self.assertIn(b"Readable content", output)
        self.assertEqual(self.cli("local", "cat", "document.html"), original)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        raise SystemExit("Run this suite on the Windows runner, not a Unix host.")
    package = args.package.resolve()
    expected = package.with_name(package.name + ".sha256").read_text().split()[0]
    with package.open("rb") as source:
        assert hashlib.file_digest(source, "sha256").hexdigest() == expected
    with tempfile.TemporaryDirectory(prefix="brains packaged 应用 ") as directory:
        root = Path(directory)
        with zipfile.ZipFile(package) as archive:
            prefix = package.stem
            assert set(archive.namelist()) == {
                f"{prefix}/{name}" for name in ("bsx.exe", "brains-server.exe", "README.md", "VERSION")}
            archive.extractall(root)
        WindowsWorkflows.binary = root / prefix / "bsx.exe"
        WindowsWorkflows.server = root / prefix / "brains-server.exe"
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(WindowsWorkflows)
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        if not result.wasSuccessful():
            raise SystemExit(1)


if __name__ == "__main__":
    main()
