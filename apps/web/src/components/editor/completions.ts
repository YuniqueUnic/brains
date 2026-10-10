import { insertCompletionText, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { buildRefPreviewDom, type RefPreviewLabels } from "./ref-preview-dom";
import { referenceStem } from "../properties/preview";

export interface CompletionFile {
  path: string;
  binary: boolean;
}

/** File references and their previews share the editor's native completion lifecycle. */
export function fileCompletions(
  files: CompletionFile[],
  onOpen: ((path: string) => void) | undefined,
  labels: RefPreviewLabels,
): (context: CompletionContext) => CompletionResult | null {
  return (context) => {
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    const wiki = /\[\[([^\[\]]*)$/.exec(before);
    const link = /(!?\[[^\]]*\]\()([^)]*)$/.exec(before);
    if (!wiki && !link) return null;
    const close = wiki ? "]]" : ")";
    return {
      from: context.pos - (wiki ? wiki[1] : link![2]).length,
      options: files.filter((file) => !wiki || !file.binary).map((file) => {
        const target = wiki ? referenceStem(file.path) : encodeURI(file.path).replace(/[()]/g, (char) => `%${char.charCodeAt(0).toString(16)}`);
        return {
          label: (wiki ? referenceStem(file.path) : file.path).split("/").pop()!,
          detail: file.path,
          type: "file",
          info: () => buildRefPreviewDom(file.path, onOpen, labels),
          apply(view, _completion, from, to) {
            // Consume the existing auto-closed suffix; never leave `))`/`]]]]`.
            const line = view.state.doc.lineAt(to);
            const closing = view.state.sliceDoc(to, line.to).indexOf(close);
            let end = closing < 0 ? to : to + closing;
            for (const char of close) {
              if (view.state.sliceDoc(end, end + 1) === char) end += 1;
            }
            view.dispatch(insertCompletionText(view.state, target + close, from, end));
          },
        };
      }),
      validFor: wiki ? /^[^\[\]]*$/ : /^[^)]*$/,
    };
  };
}
