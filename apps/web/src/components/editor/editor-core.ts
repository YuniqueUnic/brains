import { EditorState as CodeMirrorState } from "@codemirror/state";
import { EditorView, keymap, tooltips } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import {
  defaultKeymap,
  history,
  historyKeymap,
  redoDepth,
  undoDepth,
} from "@codemirror/commands";
import {
  highlightSelectionMatches,
  search,
  searchKeymap,
} from "@codemirror/search";
import { createFindPanel, type FindPanelLabels } from "./find-panel";
import { type SaveTone } from "../SaveIndicator";

/*
 * Editor core (DESIGN.md §26-34): the document is the star — 860px measure
 * (CSS), document typography, restrained markdown highlighting. Shared by
 * EditorPane and the conflict merge views so every surface edits markdown
 * with the same state machine and the same extensions.
 *
 * Draft/publish model: typing keeps a LOCAL draft (debounced localStorage,
 * crash-safe); ⌘S stashes it; only 发布 (button / ⌘⏎) writes a new revision.
 * State machine (design §13.2): loading → clean → dirty → saving
 * → clean | conflict | error.
 */

export type EditorState =
  | { kind: "empty" }
  | { kind: "loading"; path: string }
  | {
      kind: "clean" | "dirty" | "error";
      path: string;
      text: string;
      baseText: string;
      baseRevision: string;
      message?: string;
    }
  | {
      kind: "saving";
      path: string;
      /** Keystrokes during the flight buffer here; the closure's `current.text` is what was sent. */
      text: string;
      baseText: string;
      baseRevision: string;
      operationId: string;
    }
  | {
      kind: "conflict";
      path: string;
      localText: string;
      baseText: string;
      remoteRevision: string;
      remoteText: string;
      /** Latest failure inside the conflict view (kept local, network…). */
      notice?: string;
    };

export function freshEditorState(
  path: string,
  text: string,
  baseRevision: string,
): EditorState {
  return { kind: "clean", path, text, baseText: text, baseRevision };
}

/** Traffic-light tone for the publish capsule; labels stay the e2e contract. */
export function statusTone(state: EditorState): SaveTone {
  switch (state.kind) {
    case "empty":
    case "loading":
      return "neutral";
    case "clean":
      return "saved";
    case "saving":
    case "dirty":
      return "pending";
    case "conflict":
    case "error":
      return "error";
  }
}

/** Draft/publish model: clean = everything published; dirty = local draft. */
export function statusLabel(state: EditorState, t: (key: string) => string): string {
  switch (state.kind) {
    case "empty":
      return t("statusEmpty");
    case "loading":
      return t("statusLoading");
    case "clean":
      return t("statusSaved");
    case "dirty":
      return t("statusDirty");
    case "saving":
      return t("statusSaving");
    case "conflict":
      return t("statusConflict");
    case "error":
      return state.message ?? t("statusFailed");
  }
}

export function currentDoc(state: EditorState): string {
  if (state.kind === "empty" || state.kind === "loading") return "";
  if (state.kind === "conflict") return state.localText;
  return state.text;
}

export function docKey(state: EditorState): string {
  // Re-sync the CodeMirror doc when the underlying version identity changes
  // (file switch, conflict resolution, restore) but not on every keystroke.
  switch (state.kind) {
    case "empty":
      return "empty";
    case "loading":
      return `loading:${state.path}`;
    case "conflict":
      return `conflict:${state.path}:${state.remoteRevision}`;
    default:
      return `edit:${state.path}:${state.baseRevision}`;
  }
}

/* Markdown highlighting (DESIGN.md §31): near-monochrome, quiet accents. */
export const markdownHighlights = HighlightStyle.define([
  { tag: tags.heading1, fontWeight: "650", fontSize: "1.7em", lineHeight: "1.4" },
  { tag: tags.heading2, fontWeight: "630", fontSize: "1.35em", lineHeight: "1.45" },
  { tag: tags.heading3, fontWeight: "600", fontSize: "1.12em" },
  { tag: tags.heading4, fontWeight: "600" },
  { tag: tags.strong, fontWeight: "620" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through", color: "var(--muted-foreground)" },
  { tag: tags.link, color: "var(--primary)", textDecoration: "underline" },
  { tag: tags.url, color: "var(--muted-foreground)" },
  { tag: tags.monospace, class: "cm-md-code" },
  { tag: tags.quote, color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: tags.processingInstruction, color: "var(--faint-foreground)" },
  { tag: tags.meta, color: "var(--faint-foreground)" },
  { tag: [tags.keyword, tags.atom, tags.bool], color: "var(--primary)" },
  { tag: tags.comment, color: "var(--faint-foreground)" },
  { tag: [tags.string, tags.number], color: "color-mix(in srgb, var(--foreground) 78%, var(--primary))" },
]);

export function findPanelLabels(t: (key: string) => string): FindPanelLabels {
  return {
    find: t("findPlaceholder"),
    replace: t("replacePlaceholder"),
    replaceButton: t("replaceAction"),
    replaceAllButton: t("replaceAllAction"),
    previous: t("findPrevious"),
    next: t("findNext"),
    close: t("close"),
    caseSensitive: t("matchCase"),
    useRegex: t("useRegex"),
    toggleReplace: t("toggleReplace"),
  };
}

export function baseExtensions(
  onDocChange: (text: string) => void,
  onHistoryChange: (canUndo: boolean, canRedo: boolean) => void,
  findLabels: () => FindPanelLabels,
) {
  return [
    markdown({ base: markdownLanguage }),
    syntaxHighlighting(markdownHighlights),
    // Wrapping is owned by wrapComp (the preference must be able to turn it
    // off) — no static EditorView.lineWrapping here.
    history(),
    search({ top: true, createPanel: (view) => createFindPanel(findLabels())(view) }),
    // Tooltips mount on <body>: the inverse-zoomed portal layer keeps CM's
    // visual-rect math 1:1 at non-default interface sizes.
    tooltips({ parent: document.body }),
    highlightSelectionMatches(),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) onDocChange(update.state.doc.toString());
      onHistoryChange?.(undoDepth(update.state) > 0, redoDepth(update.state) > 0);
    }),
  ];
}

/** Re-export for panes that build auxiliary editors from scratch. */
export { CodeMirrorState };
