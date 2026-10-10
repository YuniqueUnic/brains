import { StateField } from "@codemirror/state";
import { closeHoverTooltips, Decoration, EditorView, hoverTooltip, keymap } from "@codemirror/view";
import { parseMarkdownDocument } from "../../lib/document-metadata";

/** One document-derived value owns both marks and source-position lookups. */
export const documentMetadata = StateField.define({
  create: (state) => parseMarkdownDocument(state.doc.toString()),
  update: (value, transaction) =>
    transaction.docChanged
      ? parseMarkdownDocument(transaction.newDoc.toString())
      : value,
  provide: (field) => EditorView.decorations.from(field, (document) =>
    Decoration.set((document.frontmatter?.tags ?? []).map(({ from, to, tag }) =>
      Decoration.mark({
        class: "cm-metadata-tag",
        attributes: { "data-tag": tag },
      }).range(from, to),
    )),
  ),
});

/** Preserve editable YAML; only the explicit hover action opens tag search. */
export function metadataTags(
  onTag: ((tag: string) => void) | undefined,
  label: (tag: string) => string,
) {
  return [
    documentMetadata,
    hoverTooltip((view, pos) => {
      if (!onTag) return null;
      const tag = view.state.field(documentMetadata).frontmatter?.tags.find(
        (range) => pos >= range.from && pos < range.to,
      );
      if (!tag) return null;
      return {
        pos: tag.from,
        end: tag.to,
        above: true,
        create() {
          const dom = document.createElement("button");
          dom.type = "button";
          dom.className = "metadata-tag-action";
          dom.textContent = label(tag.tag);
          dom.addEventListener("click", () => {
            view.dispatch({ effects: closeHoverTooltips });
            onTag(tag.tag);
          });
          return { dom };
        },
      };
    }, { hoverTime: 350, hideOnChange: true }),
    keymap.of([{
      key: "Alt-Enter",
      run(view) {
        if (!onTag || !view.state.selection.main.empty) return false;
        const pos = view.state.selection.main.head;
        const tag = view.state.field(documentMetadata).frontmatter?.tags.find(
          (range) => pos >= range.from && pos < range.to,
        );
        if (!tag) return false;
        onTag(tag.tag);
        return true;
      },
    }]),
  ];
}
