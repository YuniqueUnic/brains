import { EditorView } from "@codemirror/view";
import { startCompletion } from "@codemirror/autocomplete";
import { EditorSelection } from "@codemirror/state";
import {
  Bold,
  Code,
  Heading1,
  Heading2,
  Heading3,
  Image,
  Italic,
  Link,
  List,
  ListOrdered,
  ListTodo,
  Minus,
  Quote,
  SquareCode,
  Strikethrough,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { Tooltip } from "../ui/tooltip";
import { cn } from "../../lib/utils";

/*
 * Markdown quick-format bar (the collapsible sub-header): buttons run pure
 * text transforms on the main selection — wrap markers toggle (selected
 * `**text**` unwraps), line prefixes switch (h2 over an h1 line becomes h1)
 * and every action is ONE transaction, so ⌘Z reverts it whole.
 */

type WrapAction = { kind: "wrap"; marker: string; tail?: string };
type PrefixAction = { kind: "prefix"; token: string; match: RegExp };
type ReferenceAction = { kind: "reference"; image: boolean };
type BlockAction = { kind: "fence" } | { kind: "hr" };
export type MarkdownAction = WrapAction | PrefixAction | BlockAction | ReferenceAction;

const STRIP_MARKER = /^(#{1,6}\s|>\s|[-*+]\s(?:\[[ x]\]\s)?|\d+\.\s)?/;

const ACTIONS = {
  h1: { kind: "prefix", token: "# ", match: /^#\s/ },
  h2: { kind: "prefix", token: "## ", match: /^##\s/ },
  h3: { kind: "prefix", token: "### ", match: /^###\s/ },
  bullet: { kind: "prefix", token: "- ", match: /^[-*+]\s/ },
  ordered: { kind: "prefix", token: "1. ", match: /^\d+\.\s/ },
  todo: { kind: "prefix", token: "- [ ] ", match: /^[-*+]\s\[[ x]\]\s/ },
  quote: { kind: "prefix", token: "> ", match: /^>\s/ },
  bold: { kind: "wrap", marker: "**" },
  italic: { kind: "wrap", marker: "*" },
  strike: { kind: "wrap", marker: "~~" },
  code: { kind: "wrap", marker: "`" },
  link: { kind: "reference", image: false },
  image: { kind: "reference", image: true },
  fence: { kind: "fence" },
  hr: { kind: "hr" },
} satisfies Record<string, MarkdownAction>;

/** Apply one action to the main selection as a single transaction. */
export function applyMarkdownAction(view: EditorView, action: MarkdownAction) {
  const { state } = view;
  const range = state.selection.main;
  if (action.kind === "reference") {
    const text = state.sliceDoc(range.from, range.to);
    const label = `${action.image ? "!" : ""}[${text}](`;
    view.dispatch({ changes: { from: range.from, to: range.to, insert: `${label})` }, selection: EditorSelection.cursor(range.from + label.length) });
    view.focus();
    startCompletion(view);
    return;
  }
  if (action.kind === "hr") {
    // The divider belongs on its own line: never split the line the cursor
    // sits in — append below it.
    const line = state.doc.lineAt(range.head);
    const insert = line.text.trim() === "" ? "---\n" : "\n---\n";
    const at = line.text.trim() === "" ? line.from : line.to;
    view.dispatch({
      changes: { from: at, insert },
      selection: EditorSelection.cursor(at + insert.length),
    });
    view.focus();
    return;
  }
  if (action.kind === "fence") {
    // Fences are block-level: align the insert to whole lines so a cursor in
    // the middle of a paragraph never produces inline ``` runs.
    const text = state.sliceDoc(range.from, range.to);
    const openLine = state.doc.lineAt(range.from);
    const closeLine = state.doc.lineAt(range.to);
    const pre = range.from === openLine.from ? "" : "\n";
    const post = range.to === closeLine.to ? "" : "\n";
    const insert = `${pre}\`\`\`\n${text}\n\`\`\`${post}`;
    view.dispatch({
      changes: { from: range.from, to: range.to, insert },
      selection: EditorSelection.cursor(range.from + pre.length + 4),
    });
    view.focus();
    return;
  }
  if (action.kind === "wrap") {
    const text = state.sliceDoc(range.from, range.to);
    const head = action.marker;
    const tail = action.tail ?? action.marker;
    // Toggle: an exact wrap unwraps; anything else wraps (cursor slips
    // between the markers so typing the label/link is one keystroke away).
    if (text.startsWith(head) && text.endsWith(tail) && text.length >= head.length + tail.length) {
      const inner = text.slice(head.length, text.length - tail.length);
      view.dispatch({
        changes: { from: range.from, to: range.to, insert: inner },
        selection: EditorSelection.range(range.from, range.from + inner.length),
      });
    } else {
      view.dispatch({
        changes: { from: range.from, to: range.to, insert: `${head}${text}${tail}` },
        selection:
          text === ""
            ? EditorSelection.cursor(range.from + head.length)
            : EditorSelection.range(range.from + head.length, range.from + head.length + text.length),
      });
    }
    view.focus();
    return;
  }
  // Line-prefix toggle across the selection's lines: every line loses its
  // current marker first; lines that did not carry THIS action's marker gain
  // it (so h2 over h1 becomes h1, re-clicking the same level unwraps).
  const first = state.doc.lineAt(range.from);
  const last = state.doc.lineAt(range.empty ? range.to : range.to - 1);
  const changes = [];
  for (let n = first.number; n <= last.number; n++) {
    const line = state.doc.line(n);
    const stripped = line.text.replace(STRIP_MARKER, "");
    const had = action.match.test(line.text);
    const insert = had && line.text === action.token + stripped ? "" : action.token;
    changes.push({ from: line.from, to: line.from + line.text.length - stripped.length, insert });
  }
  const changeSet = state.changes(changes);
  view.dispatch({ changes: changeSet, selection: range.empty ? EditorSelection.cursor(changeSet.mapPos(range.head, 1)) : state.selection.map(changeSet) });
  view.focus();
}

/** The collapsible formatting row under the editor header. */
export function MarkdownToolbar({ view }: { view: EditorView | null }) {
  const { t } = useTranslation("editor");
  const run = (action: MarkdownAction) => () => {
    if (view) applyMarkdownAction(view, action);
  };
  const groups: { icon: typeof Bold; label: string; action: MarkdownAction }[][] = [
    [
      { icon: Heading1, label: t("toolbarH1"), action: ACTIONS.h1 },
      { icon: Heading2, label: t("toolbarH2"), action: ACTIONS.h2 },
      { icon: Heading3, label: t("toolbarH3"), action: ACTIONS.h3 },
    ],
    [
      { icon: Bold, label: t("toolbarBold"), action: ACTIONS.bold },
      { icon: Italic, label: t("toolbarItalic"), action: ACTIONS.italic },
      { icon: Strikethrough, label: t("toolbarStrike"), action: ACTIONS.strike },
      { icon: Code, label: t("toolbarCode"), action: ACTIONS.code },
    ],
    [
      { icon: List, label: t("toolbarBullet"), action: ACTIONS.bullet },
      { icon: ListOrdered, label: t("toolbarOrdered"), action: ACTIONS.ordered },
      { icon: ListTodo, label: t("toolbarTodo"), action: ACTIONS.todo },
      { icon: Quote, label: t("toolbarQuote"), action: ACTIONS.quote },
    ],
    [
      { icon: Link, label: t("toolbarLink"), action: ACTIONS.link },
      { icon: Image, label: t("toolbarImage"), action: ACTIONS.image },
      { icon: SquareCode, label: t("toolbarFence"), action: ACTIONS.fence },
      { icon: Minus, label: t("toolbarHr"), action: ACTIONS.hr },
    ],
  ];
  return (
    <div className="flex h-8 shrink-0 items-center border-b px-2" data-testid="markdown-toolbar">
      {/* Keep all formatting actions reachable on narrow screens. */}
      <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto">
        {groups.map((group, index) => (
            <span key={index} className="flex shrink-0 items-center gap-0.5">
              {index > 0 && <span className="mx-1 h-4 w-px shrink-0 bg-border" />}
              {group.map(({ icon: Icon, label, action }) => (
                <Tooltip key={label} content={label}>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={label}
                    disabled={!view}
                    onClick={run(action)}
                  >
                    <Icon className={cn("size-3.5 stroke-[1.5]")} />
                  </Button>
                </Tooltip>
              ))}
            </span>
          ))}
      </div>

    </div>
  );
}
