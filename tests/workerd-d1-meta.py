"""D1-specific business verification for #13 meta filters and #18 hybrid search.

Run via scripts/run-workerd.py: BRAINS_BASE_URL / BRAINS_E2E_TOKEN point at an
isolated workerd with a fresh D1 database, so every assertion below executes
the real D1 SQL bindings (js_str_or_null / js_num_or_null) that turso-path
tests cannot exercise — in particular the regression where the CLI posted the
application `ListQuery` directly and the wire contract silently dropped the
`type` filter.

Exit codes honored (see `bsx search --help`): 1 = zero hits, 7 = incomplete.
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cli_support import Cli

cli = Cli(root=Path(tempfile.mkdtemp(prefix="brains-d1-meta-")))
cli.remote("cloud", os.environ["BRAINS_BASE_URL"], os.environ["BRAINS_E2E_TOKEN"])


def indexed(*args):
    return [json.loads(line) for line in
            cli("cloud", "ls", *args, "--source", "index", "--json").splitlines()]


def search_raw(*args, expected=0):
    result = subprocess.run(
        [str(cli.binary), "--profile", "cloud", "search", *args, "--json"],
        capture_output=True, env=cli.env, timeout=120,
    )
    assert result.returncode == expected, (
        args, result.returncode, result.stdout.decode(errors="replace"),
        result.stderr.decode(errors="replace"))
    return result


def search_hits(*args):
    return [json.loads(line) for line in search_raw(*args).stdout.splitlines()]


CLI_DECS = ("---\nstatus: draft\ntype: Decision\ntags: [d1, meta]\n"
            "stale_after: \"2099-01-01\"\n---\n# D1 meta filters\n\n"
            "Draft decision about worfklast filters and hybrid ranking.\n").encode()
MD_STABLE = ("---\nstatus: stable\ntype: Reference\ntags: [stable]\n"
             "verified:\n  - by: \"human:alice\"\n    at: \"2026-10-01\"\n---\n"
             "# Stable reference\n\nSettled reference material.\n").encode()
MD_OLD = ("---\nstatus: draft\ntags: [d1]\nstale_after: \"2020-01-01\"\n---\n"
          "# Old draft\n\nA stale draft note.\n").encode()
cli("cloud", "put", "decisions/ship-d1.md", "--create", data=CLI_DECS)
cli("cloud", "put", "notes/stable-ref.md", "--create", data=MD_STABLE)
cli("cloud", "put", "notes/old-draft.md", "--create", data=MD_OLD)

# --status must filter exactly on D1: the bound-empty regression returned the
# whole vault for draft and nothing for stable.
assert {e["path"] for e in indexed("--status", "draft")} == {
    "decisions/ship-d1.md", "notes/old-draft.md"}
assert [e["path"] for e in indexed("--status", "stable")] == ["notes/stable-ref.md"]
assert indexed("--status", "deprecated") == []
assert [e["path"] for e in indexed("--status", "draft", "--type", "Decision")] == [
    "decisions/ship-d1.md"]
assert [e["path"] for e in indexed("--type", "Reference")] == ["notes/stable-ref.md"]

# Trust tiers derive from verified entries.
assert [e["path"] for e in indexed("--trust", "human-reviewed")] == ["notes/stable-ref.md"]
assert {e["path"] for e in indexed("--trust", "unverified")} == {
    "decisions/ship-d1.md", "notes/old-draft.md"}

# Staleness is computed against the filter's date; fresh includes notes
# without any stale_after.
assert [e["path"] for e in indexed("--stale")] == ["notes/old-draft.md"]
assert {e["path"] for e in indexed("--fresh")} == {
    "decisions/ship-d1.md", "notes/stable-ref.md"}

# FTS with a tag filter: the tag subclause must bind on D1 (the regression
# dropped it silently, leaking untagged matches). Zero hits exit 1.
hits = search_hits("worfklast", "--tag", "meta", "--mode", "fts")
assert [h["path"] for h in hits] == ["decisions/ship-d1.md"]
leak = search_raw("reference", "--tag", "d1", "--mode", "fts", expected=1)
assert leak.stdout == b""

# Hybrid mode on the Worker: semantic is explicitly unavailable (stderr), and
# literal+fts still return ranked hits with their retrieval provenance.
hybrid_raw = search_raw("worfklast", "--mode", "hybrid")
hybrid = [json.loads(line) for line in hybrid_raw.stdout.splitlines()]
assert [h["path"] for h in hybrid] == ["decisions/ship-d1.md"]
assert {r["retriever"] for r in hybrid[0]["retrieved_by"]} >= {"literal", "fts"}
assert "semantic" in hybrid_raw.stderr.decode(), hybrid_raw.stderr

# check runs against the remote endpoint and reports the stale note.
report = json.loads(cli("cloud", "check", "--json", expected=10))
paths = {f["path"]: f["rule"] for f in report["findings"]}
assert paths.get("notes/old-draft.md") == "stale", paths
assert report["checked"] >= 3

print("workerd D1: meta filters, trust tiers, staleness, tag-bound FTS, "
      "hybrid provenance and remote check all passed")
