import type { CompletionContext, CompletionResult } from "@codemirror/autocomplete";

export interface CompletionFile {
  path: string;
  binary: boolean;
}

/**
 * Reference completions (DESIGN.md §41 wiki links): `[[` suggests documents
 * (by stem, the [[...]] convention), `[](` and `![](` suggest every file
 * (closing the parens) so existing notes and images are one keystroke away.
 */
export function fileCompletions(
  files: CompletionFile[],
): (context: CompletionContext) => CompletionResult | null {
  const documents = files.filter((file) => !file.binary);
  const stemOf = (path: string) => path.replace(/\.[a-z0-9]+$/i, "");
  return (context) => {
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    // The class must exclude BOTH bracket kinds; the old `[^[\\]]` parsed as
    // "not [ or backslash, then a literal ]" and never matched `[[` at all.
    const wiki = /\[\[([^\[\]]*)$/.exec(before);
    if (wiki) {
      return {
        from: context.pos - wiki[1].length,
        options: documents.map((file) => {
          const stem = stemOf(file.path);
          return {
            label: stem.split("/").pop() ?? stem,
            detail: file.path,
            apply: stem,
            type: "file",
          };
        }),
        validFor: /^[^\[\]]*$/,
      };
    }
    const link = /(!?\[[^\]]*\]\()([^)]*)$/.exec(before);
    if (link) {
      return {
        from: context.pos - link[2].length,
        options: files.map((file) => ({
          label: file.path.split("/").pop() ?? file.path,
          detail: file.path,
          apply: `${file.path})`,
          type: "file",
        })),
        validFor: /^[^)]*$/,
      };
    }
    return null;
  };
}
