import { test, expect } from "@playwright/test";
import { resolveReference } from "../../../apps/web/src/components/markdown/references";
import { parseMarkdownDocument } from "../../../apps/web/src/lib/document-metadata";

test("relative links select siblings, encoded names stay intact, and ambiguous short names stay unresolved", () => {
  const files = [
    "note.md",
    "docs/note.md",
    "docs/same.md",
    "docs/same.txt",
    "docs/a#b.md",
    "docs/name:one.md",
  ].map((path) => ({ path }));
  expect(resolveReference("note.md", files, "docs/current.md")).toMatchObject({
    kind: "vault",
    path: "docs/note.md",
  });
  expect(resolveReference("same", files, "docs/current.md")).toMatchObject({
    kind: "missing",
  });
  expect(resolveReference("a%23b.md", files, "docs/current.md")).toMatchObject({
    kind: "vault",
    path: "docs/a#b.md",
  });
  expect(
    resolveReference("name%3Aone.md", files, "docs/current.md"),
  ).toMatchObject({ kind: "vault", path: "docs/name:one.md" });
  expect(
    resolveReference("../../note.md", files, "docs/current.md"),
  ).toMatchObject({ kind: "missing" });
  expect(resolveReference("#正文", files, "docs/current.md")).toEqual({
    kind: "anchor",
    fragment: "#正文",
  });
});

test("metadata keeps URL pipes, reads wiki aliases, and matches index tag rules", () => {
  const doc = parseMarkdownDocument(
    "\uFEFF---\r\ntags: ['a,b', ' rust ', rust, 42]\r\nlinks:\r\n  - https://example.com/?q=a|b\r\n  - '[[note|别名]]'\r\n---",
  );
  expect(doc.body).toBe("");
  expect(doc.metadata?.tags).toEqual(["a,b", "rust"]);
  expect(doc.metadata?.links).toEqual([
    {
      label: "https://example.com/?q=a|b",
      target: "https://example.com/?q=a|b",
    },
    { label: "别名", target: "note" },
  ]);
  const invalid = "---\ntitle: one\ntitle: two\n---\n正文";
  expect(parseMarkdownDocument(invalid)).toEqual({
    body: invalid,
    metadata: null,
    invalidMetadata: true,
  });
});
