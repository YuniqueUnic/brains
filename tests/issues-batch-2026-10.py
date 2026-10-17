#!/usr/bin/env python3
"""Business acceptance for the issue #12–#22 batch (2026-10-16).

Real bsx processes against disposable local stores: lifecycle frontmatter
(#13), bsx check (#14), checkpoint (#15), working copy (#21), OKF round trip
(#20), hybrid search (#18), root-relative links (#12), PDF conversion (#19)
and URL import ladder (#22) against a local fixture server.
"""
import json
import subprocess
import sys
import tempfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import threading

REPO = Path(__file__).resolve().parent.parent
CLI = REPO / "target/debug/bsx"
PDF = Path("/tmp/xberg_probe/real.pdf")
assert CLI.exists(), "build bsx first"
assert PDF.exists(), "fixture PDF missing"


class Cli:
    def __init__(self, root):
        self.config = root / "config.json"
        self.env = {k: v for k, v in __import__("os").environ.items() if k not in ("BRAIN_CONFIG",)}
        self.env["BRAIN_CONFIG"] = str(self.config)

    def __call__(self, profile, *args, data=None, expected=0):
        result = subprocess.run(
            [str(CLI), "--profile", profile, *map(str, args)],
            input=data, capture_output=True, env=self.env, timeout=120,
        )
        assert result.returncode == expected, (
            args, result.returncode, result.stdout.decode(errors="replace")[:800],
            result.stderr.decode(errors="replace")[:800],
        )
        return result.stdout

    def stat(self, profile, path):
        return json.loads(self(profile, "stat", path, "--json"))

    def put(self, profile, path, data):
        self(profile, "put", path, "--if-revision", self.stat(profile, path)["revision"], data=data)


def check(name, condition, detail=""):
    status = "PASS" if condition else "FAIL"
    print(f"[{status}] {name}" + (f" — {detail}" if detail and not condition else ""))
    if not condition:
        sys.exit(1)


def start_fixture_server(handler, port=0):
    server = ThreadingHTTPServer(("127.0.0.1", port), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, f"http://127.0.0.1:{server.server_address[1]}"


def main():
    with tempfile.TemporaryDirectory(prefix="brains-issues-batch-") as directory:
        root = Path(directory)
        cli = Cli(root)
        data = root / "data"
        cli("local", "init", data)

        # ---- #13: lifecycle/trust/provenance frontmatter parses and filters ----
        note = """---
type: Decision
title: 缓存策略
description: 部署缓存的决定
status: draft
stale_after: "2099-01-01"
generated:
  by: tool/agent-x/1.0
  at: "2026-10-01T09:00:00Z"
verified:
  - by: human:alice
    at: "2026-10-02T10:00:00Z"
sources:
  - id: run-1
    resource: evidence/run-1.md
    last_modified: "2026-09-30"
tags: [cache]
---

正文引用[^run-1]。

[^run-1]: 见 sources。
"""
        cli("local", "put", "notes/decision.md", "--create", data=note.encode())
        listed = [json.loads(line) for line in
                  cli("local", "ls", "notes/", "--source", "index", "--status", "draft", "--type", "Decision",
                      "--trust", "human-reviewed", "--json").splitlines()]
        check("#13 status/type/trust 过滤命中", len(listed) == 1 and listed[0]["path"] == "notes/decision.md")
        meta = listed[0].get("meta") or {}
        check("#13 meta 投影含 status/trust/stale_after", meta.get("status") == "draft" and meta.get("trust") == "human-reviewed" and meta.get("stale_after") == "2099-01-01", str(meta))
        fresh = cli("local", "ls", "notes/", "--source", "index", "--stale", "--json", expected=0)
        stale_list = [json.loads(line) for line in fresh.splitlines()] if fresh.strip() else []
        check("#13 --stale 不含未过期笔记", all(row["path"] != "notes/decision.md" for row in stale_list))

        # ---- #12: root-relative links resolve to vault targets ----
        cli("local", "put", "notes/sales/q3.md", "--create", data=b"[t](/tables/orders.md)")
        cli("local", "put", "tables/orders.md", "--create", data=b"orders")
        links = [json.loads(line) for line in cli("local", "links", "notes/sales/q3.md", "--json").splitlines()]
        check("#12 根相对链接解析到 /tables/orders.md", links and links[0]["target"] == "tables/orders.md", str(links))
        cli("local", "put", "notes/sales/escape.md", "--create", data=b"[e](/../out.md)")
        escape = [json.loads(line) for line in cli("local", "links", "notes/sales/escape.md", "--json").splitlines()]
        check("#12 /../ 逃逸记为 outside", escape and escape[0]["kind"] == "outside", str(escape))

        # PARSER_VERSION 4: reindex refreshes edges
        cli("local", "reindex", "--all-pages")
        links = [json.loads(line) for line in cli("local", "links", "notes/sales/q3.md", "--json").splitlines()]
        check("#12 reindex 后链接边仍正确", links and links[0]["target"] == "tables/orders.md")

        # ---- #18: hybrid search fuses literal + fts and reports availability ----
        hits = [json.loads(line) for line in
                cli("local", "search", "缓存", "--mode", "hybrid", "--json").splitlines()]
        check("#18 hybrid 命中带 retrieved_by", hits and hits[0]["retrieved_by"], str(hits))
        fused = hits[0]["retrieved_by"]
        check("#18 retrieved_by 标注检索器", all("retriever" in r and "rank" in r for r in fused), str(fused))

        # ---- #14: bsx check rules ----
        # machine-generated, unverified, no sources → findings
        inferred_note = "---\ntype: Note\ngenerated:\n  by: tool/agent-x/1.0\n---\n\n推断：缓存命中率会上升。\n"
        cli("local", "put", "notes/inferred.md", "--create", data=inferred_note.encode())
        report = json.loads(cli("local", "check", "--json", expected=10))
        rules = {f["rule"] for f in report["findings"]}
        paths = {f["path"] for f in report["findings"]}
        check("#14 未确认机器笔记 reported", "unconfirmed" in rules, str(rules))
        check("#14 缺 sources/confidence/stale_after reported",
              any("inferred-unsupported" in r for r in rules), str(rules))
        cli("local", "put", "notes/broken.md", "--create", data=b"[missing](/missing.md)")
        report = json.loads(cli("local", "check", "--json", expected=10))
        rules = {f["rule"] for f in report["findings"]}
        paths = {f["path"] for f in report["findings"]}
        check("#14 断链报告 link-unresolved",
              "link-unresolved" in rules and "notes/broken.md" in paths, str(rules))
        cli("local", "check", "--prefix", "tables/", expected=0)  # clean subtree exits 0

        # ---- #15: checkpoint show/update with CAS ----
        checkpoint_v1 = "# Checkpoint\n\n## 下一步\n- 复查缓存\n"
        cli("local", "checkpoint", "update", "notes", "--create", data=checkpoint_v1.encode())
        shown = cli("local", "checkpoint", "show", "notes")
        check("#15 checkpoint show 读回", "复查缓存".encode() in shown)
        rev = cli.stat("local", "notes/checkpoint.md")["revision"]
        cli("local", "checkpoint", "update", "notes", "--if-revision", rev, data="# Checkpoint v2\n".encode())
        check("#15 条件写成功", "Checkpoint v2".encode() in cli("local", "checkpoint", "show", "notes"))
        old = json.loads(cli("local", "history", "notes/checkpoint.md", "--json").splitlines()[1])["revision"]
        cli("local", "restore", "notes/checkpoint.md", "--revision", old,
            "--if-revision", cli.stat("local", "notes/checkpoint.md")["revision"])
        check("#15 可恢复旧修订", "复查缓存".encode() in cli("local", "checkpoint", "show", "notes"))

        # ---- #21: tracked working copy ----
        wc = root / "workcopy"
        cli("local", "checkout", str(wc))
        check("#21 checkout 写 manifest", (wc / ".brain/working-copy.json").is_file())
        (wc / "notes/local-edit.md").write_text("local only\n", encoding="utf-8")
        # concurrent change on the store side
        cli.put("local", "notes/decision.md", note.replace("部署缓存", "部署缓存 v2").encode())
        status = json.loads(cli("local", "status", str(wc), "--json"))
        rows = {r["path"]: r for r in status["rows"]}
        check("#21 status 识别本地新增", rows.get("notes/local-edit.md", {}).get("local") == "added", str(rows))
        check("#21 status 识别 store 修改", rows.get("notes/decision.md", {}).get("store") == "modified", str(rows))
        push = json.loads(cli("local", "push", str(wc), "--dry-run", "--json")) if False else None
        # push with a store-side conflict reports conflict, exit 11
        # overlapping same-line edit: push must conflict, never overwrite
        (wc / "notes/decision.md").write_text(note.replace("决定", "决定(本地)"), encoding="utf-8")
        proc = subprocess.run([str(CLI), "--profile", "local", "push", str(wc)],
                              capture_output=True, env=cli.env, timeout=60)
        check("#21 push 双方修改 → 冲突退出码 11", proc.returncode == 11 and b"notes/decision.md" in proc.stderr, f"rc={proc.returncode} err={proc.stderr[:200]}")
        # non-overlapping local edit on a different line: pull three-way merges
        (wc / "notes/decision.md").write_text(note + "\n本地补充行：合并验证。\n", encoding="utf-8")
        pull = json.loads(cli("local", "pull", str(wc), "--json", expected=0))
        check("#21 pull 三方合并成功", pull["merged"] == 1 and "本地补充行" in (wc / "notes/decision.md").read_text() and "v2" in (wc / "notes/decision.md").read_text(), str(pull))
        # simple fast-forward pull: clean file updated in store only
        cli("local", "put", "notes/fastfwd.md", "--create", data=b"new in store\n")
        pull2 = json.loads(cli("local", "pull", str(wc), "--json"))
        check("#21 pull 快进新文件", (wc / "notes/fastfwd.md").read_text() == "new in store\n", str(pull2))
        # clean push of the local-only file
        (wc / "notes/decision.md").write_text(cli("local", "cat", "notes/decision.md").decode())
        proc = subprocess.run([str(CLI), "--profile", "local", "push", str(wc)],
                              capture_output=True, env=cli.env, timeout=60)
        check("#21 push 收敛无冲突", proc.returncode == 0, f"rc={proc.returncode} {proc.stdout[:200]}{proc.stderr[:200]}")
        check("#21 push 后 store 有该文件", cli.stat("local", "notes/local-edit.md")["path"] == "notes/local-edit.md")

        # ---- #20: OKF export → import → export round trip ----
        vault_src = root / "okf-export-1"
        cli("local", "export", str(vault_src), "--format", "okf", "--with-log")
        check("#20 根 index.md 生成", (vault_src / "index.md").is_file())
        check("#20 log.md 生成", (vault_src / "log.md").is_file())
        root_index = (vault_src / "index.md").read_text(encoding="utf-8")
        check("#20 根 index 声明 okf_version", 'okf_version: "0.2"' in root_index, root_index[:120])
        notes_index = (vault_src / "notes/index.md").read_text(encoding="utf-8")
        check("#20 index 用根相对链接", "](/notes/decision.md)" in notes_index, notes_index[:200])
        # import into an empty vault
        data2 = root / "data2"
        cli("remote", "init", data2)
        cli("remote", "import", str(vault_src), "--format", "okf")
        vault_src2 = root / "okf-export-2"
        cli("remote", "export", str(vault_src2), "--format", "okf", "--with-log")
        # concept files byte-identical (ignore generated reserved files)
        mismatch = []
        for f in vault_src.rglob("*"):
            if f.is_file() and f.name not in ("index.md", "log.md"):
                rel = f.relative_to(vault_src)
                other = vault_src2 / rel
                if not other.exists() or other.read_bytes() != f.read_bytes():
                    mismatch.append(str(rel))
        check("#20 round-trip 概念文件逐字节一致", not mismatch, str(mismatch[:5]))
        # index regeneration
        cli("local", "index-scope", "notes")
        idx = cli("local", "cat", "notes/index.md")
        check("#20 bsx index 再生成含标题", b"Index" in idx and b"decision.md" in idx)

        # ---- #19: convert attachment → companion note ----
        cli("local", "put", "docs/report.pdf", "--create", data=PDF.read_bytes())
        caps = json.loads(cli("local", "capabilities", "--json"))
        conv = caps.get("document_conversion")
        check("#19 capabilities 声明 document_conversion", conv and conv["converter"] == "xberg", str(conv))
        cli("local", "convert", "docs/report.pdf")
        companion = cli("local", "cat", "docs/report.pdf.md")
        check("#19 companion frontmatter type/status/source", b"type: Converted Document" in companion and b"status: draft" in companion and b"id: original" in companion)
        check("#19 正文含 PDF 文本", b"APPLE INC." in companion)
        # re-convert of an unchanged original writes nothing; a changed
        # original produces a new companion revision on the same path
        gen_before = cli.stat("local", "docs/report.pdf.md")["generation"]
        cli("local", "convert", "docs/report.pdf")
        gen_same = cli.stat("local", "docs/report.pdf.md")["generation"]
        check("#19 重转换无变化不产生新修订", gen_same == gen_before)
        original = cli.stat("local", "docs/report.pdf")
        check("#19 原始附件未改", original["size"] == len(PDF.read_bytes()))
        cli("local", "put", "docs/data.csv", "--create", data=b"a,b\n1,2\n")
        cli("local", "convert", "docs/data.csv")
        gen_csv = cli.stat("local", "docs/data.csv.md")["generation"]
        rev = cli.stat("local", "docs/data.csv")["revision"]
        cli("local", "put", "docs/data.csv", "--if-revision", rev, data=b"a,b\n1,2\n3,4\n")
        cli("local", "convert", "docs/data.csv")
        check(
            "#19 原始变更后重转换出新修订",
            cli.stat("local", "docs/data.csv.md")["generation"] == gen_csv + 1,
        )

        # ---- #22: import-url ladder against a local fixture ----
        # NOTE: http is refused by default; --allow-http allows this localhost fixture,
        # and --allow-host 127.0.0.1 exempts loopback explicitly.
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path == "/robots.txt":
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b"User-agent: *\nDisallow:\n")
                elif self.path == "/page.md":
                    body = b"# Negotiated\n\n served as markdown\n"
                    self.send_response(200)
                    self.send_header("Content-Type", "text/markdown")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                elif self.path == "/page.html":
                    body = ("<html><head><title>Static</title></head><body><article>"
                            "<h1>Deep正文</h1><p>" + "很长的正文内容。" * 40 + "</p></article></body></html>").encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "text/html; charset=utf-8")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                else:
                    self.send_response(404)
                    self.end_headers()

            def log_message(self, *args):
                pass

        server, base = start_fixture_server(Handler)
        try:
            # SSRF defaults: http refused
            proc = subprocess.run([str(CLI), "--profile", "local", "import-url",
                                   f"{base}/page.md", "web/negotiated.md"],
                                  capture_output=True, env=cli.env, timeout=60)
            check("#22 默认拒绝 http", proc.returncode != 0 and b"not allowed" in proc.stderr, proc.stderr.decode()[:200])
            # markdown negotiation
            out = cli("local", "import-url", f"{base}/page.md", "web/negotiated.md",
                      "--allow-http", "--allow-host", "127.0.0.1")
            check("#22 阶梯1 协商 markdown", b"markdown-negotiation" in out, out.decode()[:200])
            body = cli("local", "cat", "web/negotiated.md")
            check("#22 正文与溯源", b"served as markdown" in body and b"id: page" in body and b"status: draft" in body)
            # static html
            out = cli("local", "import-url", f"{base}/page.html", "web/static.md",
                      "--allow-http", "--allow-host", "127.0.0.1")
            check("#22 阶梯2 静态 HTML", b"static-html" in out, out.decode()[:200])
            body = cli("local", "cat", "web/static.md")
            check("#22 readability 抽取正文", b"Deep" in body, body.decode()[:200])
            # robots refused
            class Robots(BaseHTTPRequestHandler):
                def do_GET(self):
                    body = b"User-agent: *\nDisallow: /\n"
                    self.send_response(200)
                    self.send_header("Content-Type", "text/plain")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

                def log_message(self, *args):
                    pass

            server2, base2 = start_fixture_server(Robots)
            try:
                proc = subprocess.run([str(CLI), "--profile", "local", "import-url",
                                       f"{base2}/page.md", "web/blocked.md", "--allow-http",
                                       "--allow-host", "127.0.0.1"],
                                      capture_output=True, env=cli.env, timeout=60)
                check("#22 robots 默认遵守", proc.returncode != 0 and b"ROBOTS" in proc.stderr, proc.stderr.decode()[:200])
            finally:
                server2.shutdown()
        finally:
            server.shutdown()

        # seeded guides exist and mention the new workflow (#16)
        cli("local", "seed")
        guide = cli("local", "cat", "docs/ai-guide.md")
        check("#16 seed 指南含 lifecycle/checkpoint 章节", "生命周期与信任字段".encode() in guide and "Checkpoint 工作流".encode() in guide and "何时回忆".encode() in guide)

    print("ALL BUSINESS CHECKS PASSED")


if __name__ == "__main__":
    main()
