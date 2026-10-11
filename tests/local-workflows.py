#!/usr/bin/env python3
"""Business scenarios for standalone bsx, server handoff, and multiple libraries.

Build brains-cli and brains-server first. Every scenario owns disposable stores,
profiles and processes; no real user data, credentials or external service is used.
"""

import fcntl
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import unittest

from cli_support import CLI_BIN, SERVER_BIN, Cli, REPO, exercise_repeated_relations, native_server


class LocalWorkflows(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="brains-local-business-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.data = self.root / "offline"
        self.cli = Cli(self.root)
        self.cli("local", "init", self.data)

    def rows(self, profile, *args):
        return [json.loads(line) for line in self.cli(profile, *args).splitlines()]

    def test_build_identity_is_reported_by_the_running_instance(self):
        """Given CLI/server binaries, version and instance reports agree without starting on --version."""
        version_dir = self.root / "version-only"
        version_dir.mkdir()
        versions = {}
        for name, binary in (("bsx", CLI_BIN), ("brains-server", SERVER_BIN)):
            output = subprocess.check_output(
                [str(binary.resolve()), "--version"], cwd=version_dir,
                env=self.cli.env, text=True, timeout=10,
            ).strip()
            self.assertTrue(output.startswith(name + " "), output)
            versions[name] = output.removeprefix(name + " ")
            self.assertRegex(versions[name], r"^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)? \(git (?:[0-9a-f]{7,12}|unknown)(?:-dirty)?\)$")
        self.assertEqual(list(version_dir.iterdir()), [])
        self.assertEqual(json.loads(self.cli("local", "capabilities", "--json"))["build_version"], versions["bsx"])
        with native_server(self.root, self.root / "server", "version-proof") as (url, token):
            self.cli.remote("server", url, token)
            caps = json.loads(self.cli("server", "capabilities", "--json"))
            self.assertEqual(caps["build_version"], versions["brains-server"])
            self.assertIn(versions["brains-server"].encode(), self.cli("server", "capabilities"))
        # A separately deployed service can have a different identity from this CLI.
        caps["build_version"] = "99.0.0 (git abcdef123456)"

        class OtherBuild(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(caps).encode())

            def log_message(self, *_):
                pass

        with ThreadingHTTPServer(("127.0.0.1", 0), OtherBuild) as server:
            thread = threading.Thread(target=server.serve_forever)
            thread.start()
            try:
                self.cli.remote("other-build", f"http://127.0.0.1:{server.server_port}", "fixture")
                remote = json.loads(self.cli("other-build", "capabilities", "--json"))
                self.assertEqual(remote["build_version"], caps["build_version"])
                self.assertIn(caps["build_version"].encode(), self.cli("other-build", "capabilities"))
            finally:
                server.shutdown()
                thread.join()

    def test_offline_documents_history_and_export_snapshot(self):
        """Given no server, import/edit/copy/move/restore preserve bytes and history."""
        cli = self.cli
        source = self.root / "notes"
        source.mkdir()
        (source / "a.md").write_text("---\ntags: [local]\n---\n# Alpha\n[Target](b.md)\n")
        (source / "b.md").write_text("# Beta\n")
        (source / "raw.bin").write_bytes(b"\x00\xff\x01\x80")
        (source / "run.sh").write_text("#!/bin/sh\necho local\n")
        (source / "run.sh").chmod(0o755)
        cli("local", "import", source, "--dry-run")
        self.assertEqual(cli("local", "ls", "--json"), b"")
        cli("local", "import", source)
        original = cli.stat("local", "a.md")["revision"]
        cli("local", "put", "a.md", "--create", data=b"reject", expected=5)
        cli("local", "patch", "a.md", "--if-revision", original,
            "--old", "Alpha", "--new", "Edited", "--dry-run")
        self.assertEqual(cli.stat("local", "a.md")["revision"], original)
        cli("local", "patch", "a.md", "--if-revision", original,
            "--old", "Alpha", "--new", "Edited")
        changed = cli.stat("local", "a.md")["revision"]
        cli("local", "put", "a.md", "--if-revision", original, data=b"stale", expected=5)
        self.assertIn(b"Edited", cli("local", "diff", "a.md", "--from", original, "--to", changed))
        self.assertEqual(cli("local", "show", "a.md", "--revision", original), (source / "a.md").read_bytes())
        cli("local", "restore", "a.md", "--revision", original, "--if-revision", changed)
        editor = self.root / "editor"
        editor.write_text('#!/bin/sh\nprintf "# Edited locally\\n" > "$1"\n')
        editor.chmod(0o755)
        cli.env["EDITOR"] = str(editor)
        cli("local", "edit", "b.md", "--if-revision", cli.stat("local", "b.md")["revision"])
        cli("local", "cp", "b.md", "copy.md", "--if-revision", cli.stat("local", "b.md")["revision"])
        cli("local", "mv", "copy.md", "moved.md", "--if-revision", cli.stat("local", "copy.md")["revision"])
        cli("local", "cat", "copy.md", expected=3)
        cli("local", "rm", "raw.bin", "--if-revision", cli.stat("local", "raw.bin")["revision"])
        self.assertEqual(len(self.rows("local", "history", "a.md", "--json")), 3)
        self.assertEqual(self.rows("local", "links", "a.md", "--json")[0]["target"], "b.md")
        self.assertEqual(self.rows("local", "links", "b.md", "--backlinks", "--json")[0]["source"], "a.md")
        self.assertEqual(len(self.rows("local", "search", "Alpha", "--tag", "local", "--json")), 1)
        cli("local", "search", "Alpha", "--mode", "fts")
        cli("local", "search", "Alpha", "--prefix", "", "--tag", "")
        self.assertEqual(cli("local", "ls", "--tag", "", "--json"), cli("local", "ls", "--json"))
        cli("local", "search", "", expected=2)
        destination = self.root / "export"
        cli("local", "export", destination)
        self.assertEqual((destination / "a.md").read_bytes(), (source / "a.md").read_bytes())
        self.assertTrue((destination / "run.sh").stat().st_mode & 0o111)
        self.assertFalse((destination / "raw.bin").exists())
        snapshot = {
            "entries": [{key: item[key] for key in ("path", "generation", "state")}
                        for item in self.rows("local", "ls", "--deleted", "--json")],
            "session": json.loads(cli("local", "whoami", "--json")),
            "exported_text": (destination / "moved.md").read_text(),
        }
        expected = json.loads((REPO / "tests/snapshots/local-workflow.json").read_text())
        self.assertEqual(snapshot, expected)
        self.assertFalse((self.data / "auth.json").exists())
        self.assertFalse((self.data / "index.db").exists())
        self.assertTrue((self.data / "vaults/default/index.turso-fts.db").is_file())

    def test_pagination_stays_in_one_local_session(self):
        """Given more than one page, list/search/history/reindex visit all records."""
        cli = self.cli
        source = self.root / "many"
        source.mkdir()
        for number in range(105):
            (source / f"{number:03}.md").write_text(f"# Match {number}\n")
        cli("local", "import", source)
        self.assertEqual(len(self.rows("local", "ls", "--json")), 105)
        self.assertEqual(len(self.rows("local", "search", "Match", "--json")), 105)
        cli("local", "search", "Match", "--max-pages", 1, expected=7)
        for number in range(21):
            cli.put("local", "000.md", f"# Match revision {number}\n".encode())
        self.assertEqual(len(self.rows("local", "history", "000.md", "--json")), 22)
        self.assertIn(b"105 files, 0 failures", cli("local", "reindex", "--all-pages", "--limit", 5))

    def test_repeated_links_and_tags_remain_indexed_after_edit(self):
        """Given repeated relations, editing a note updates search and replaces old links."""
        exercise_repeated_relations(self.cli, "local")

    def test_archive_then_collaborate_with_two_servers(self):
        """Given offline history, restore it on A, edit A, then sync local and B."""
        cli = self.cli
        cli("local", "put", "topic.md", "--create", data=b"alpha\nbeta\n")
        cli.put("local", "topic.md", b"ALPHA\nbeta\n")
        archive = self.root / "archive"
        cli("local", "export", archive, "--format", "archive")
        with native_server(self.root, self.root / "server-a", "a") as (url, token), \
             native_server(self.root, self.root / "server-b", "b") as (url_b, token_b):
            cli.remote("server-a", url, token)
            cli.remote("server-b", url_b, token_b)
            cli("server-a", "import", archive, "--format", "archive")
            self.assertEqual(cli("local", "history", "topic.md", "--json"),
                             cli("server-a", "history", "topic.md", "--json"))
            cli("local", "sync", "--remote-profile", "server-a")
            cli.put("server-a", "topic.md", b"ALPHA\nBETA\n")
            cli("local", "sync", "--remote-profile", "server-a")
            self.assertEqual(cli("local", "cat", "topic.md"), b"ALPHA\nBETA\n")
            cli("local", "sync", "--remote-profile", "server-b")
            cli.put("server-b", "topic.md", b"ALPHA\nBETA\nthird\n")
            cli("local", "sync", "--remote-profile", "server-b")
            cli("local", "sync", "--remote-profile", "server-a")
            cli("server-a", "sync", "--remote-profile", "server-b", "verify")
            cli("local", "profile", "use", "server-b", "--verify-with", "local")
            self.assertEqual(json.loads(cli.config.read_text())["active"], "server-b")
        self.assertEqual(cli("local", "cat", "topic.md"), b"ALPHA\nBETA\nthird\n")

    def test_server_owns_directory_until_stopped(self):
        """Given a local library, a server takes ownership and remote CLI can edit it."""
        cli = self.cli
        cli("local", "put", "note.md", "--create", data=b"offline")
        with native_server(self.root, self.data, "default") as (url, token):
            cli("local", "ls", expected=6)
            cli.remote("hosted", url, token)
            cli.put("hosted", "note.md", b"through server")
            self.assertEqual(cli("hosted", "cat", "note.md"), b"through server")
        self.assertEqual(cli("local", "cat", "note.md"), b"through server")

    def test_waiting_cli_excludes_second_cli_and_server(self):
        """Given a CLI waiting for stdin, aliases cannot bypass its directory lock."""
        cli = self.cli
        alias = self.root / "alias"
        alias.symlink_to(self.data, target_is_directory=True)
        cli("local", "profile", "add", "alias", "--data-dir", alias)
        writer = subprocess.Popen([str(CLI_BIN), "--profile", "local",
            "put", "held.md", "--create"], env=cli.env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            with (self.data / ".brains.lock").open("rb") as lock:
                deadline = time.monotonic() + 10
                while True:
                    self.assertIsNone(writer.poll())
                    try:
                        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        fcntl.flock(lock, fcntl.LOCK_UN)
                    except BlockingIOError:
                        break
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.01)
            cli("alias", "ls", expected=6)
            server = subprocess.run([str(SERVER_BIN),
                "--data-dir", str(alias), "--no-seed"], capture_output=True, timeout=10)
            self.assertEqual(server.returncode, 2)
            self.assertIn(b"in use", server.stderr)
            writer.communicate(b"held bytes", timeout=10)
            self.assertEqual(writer.returncode, 0)
        finally:
            if writer.poll() is None:
                writer.kill()
                writer.communicate()
        self.assertEqual(cli("alias", "cat", "held.md"), b"held bytes")

    def test_index_is_rebuilt_only_after_all_content_is_readable(self):
        """Given an old root index and a missing blob, fail then recover without history loss."""
        cli = self.cli
        cli("local", "put", "note.md", "--create", data=b"# Rebuild me\n")
        original = cli.stat("local", "note.md")
        vault = self.data / "vaults/default"
        (vault / "index.turso-fts.db").rename(self.data / "index.db")
        revision = json.loads((vault / "revisions" / f'{original["revision"]}.json').read_text())
        blob = self.data / revision["content"]["key"]
        saved = blob.read_bytes()
        blob.unlink()
        cli("local", "ls", expected=6)
        self.assertFalse((vault / "index.turso-fts.db").exists())
        blob.write_bytes(saved)
        cli("local", "search", "Rebuild")
        self.assertEqual(cli.stat("local", "note.md"), original)
        self.assertEqual(list(vault.glob("index.turso-building.db*")), [])
        indexed = self.rows("local", "ls", "--source", "index", "--json")[0]
        stored = self.rows("local", "ls", "--source", "store", "--json")[0]
        for key in ("path", "revision", "generation", "state"):
            self.assertEqual(indexed[key], stored[key])

    def test_vaults_are_isolated_and_profiles_explicit(self):
        """Given two vaults with the same path, indexes and profiles keep them separate."""
        cli = self.cli
        cli("local", "put", "same.md", "--create", data=b"# First library\n")
        cli("other", "init", self.data, "--vault", "other")
        cli("other", "put", "same.md", "--create", data=b"# Second library\n")
        cli("local", "search", "First")
        cli("local", "search", "Second", expected=1)
        cli("other", "search", "Second")
        cli("other", "search", "First", expected=1)
        cli("local", "sync", "--remote-profile", "other", expected=6)
        self.assertFalse((self.root / "sync").exists())
        cli("independent", "init", self.root / "independent")
        cli("local", "sync", "--remote-profile", "independent")
        self.assertEqual(cli("independent", "cat", "same.md"), b"# First library\n")
        cli.put("independent", "same.md", b"# Updated offline\n")
        cli("local", "sync", "--remote-profile", "independent")
        self.assertEqual(cli("local", "cat", "same.md"), b"# Updated offline\n")
        config = cli.config.read_bytes()
        cli("local", "init", self.root / "must-not-exist", expected=2)
        self.assertFalse((self.root / "must-not-exist").exists())
        self.assertEqual(cli.config.read_bytes(), config)
        cli.env["BRAINS_URL"] = "http://127.0.0.1:1"
        cli("local", "ls", expected=2)
        del cli.env["BRAINS_URL"]
        cli("local", "login", "--stdin-token", data=b"unused", expected=2)
        self.assertEqual(cli.config.stat().st_mode & 0o777, 0o600)
        cli("local", "seed")
        self.assertIn(b"0 seeded", cli("local", "seed"))

    def test_remote_partial_move_keeps_partial_exit_contract(self):
        """Given the declared 409 transfer response, report partial completion (exit 7)."""
        class PartialTransfer(BaseHTTPRequestHandler):
            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                payload = json.dumps({"copied": True, "source_trashed": False,
                    "partial": True, "operation_id": "a" * 64,
                    "target_revision": "b" * 64, "source_revision": None,
                    "error": {"code": "PRECONDITION_FAILED", "message": "source changed",
                              "request_id": "contract"}}).encode()
                self.send_response(409)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *_args):
                pass

        with ThreadingHTTPServer(("127.0.0.1", 0), PartialTransfer) as server:
            thread = threading.Thread(target=server.serve_forever)
            thread.start()
            try:
                self.cli.remote("partial", f"http://127.0.0.1:{server.server_port}", "fixture")
                self.cli("partial", "mv", "source.md", "target.md", "--if-revision", "a" * 64, expected=7)
            finally:
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    unittest.main(verbosity=2)
