import { EditorView } from "@codemirror/view";
import { EditorSelection } from "@codemirror/state";
import {
  Bold,
  ChevronDown,
  ChevronUp,
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
type BlockAction = { kind: "fence" } | { kind: "hr" };
export type MarkdownAction = WrapAction | PrefixAction | BlockAction;

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
  link: { kind: "wrap", marker: "[", tail: "]()" },
  image: { kind: "wrap", marker: "![", tail: "]()" },
  fence: { kind: "fence" },
  hr: { kind: "hr" },
} satisfies Record<string, MarkdownAction>;

/** Apply one action to the main selection as a single transaction. */
export function applyMarkdownAction(view: EditorView, action: MarkdownAction) {
  const { state } = view;
  const range = state.selection.main;
  if (action.kind === "hr") {
    const line = state.doc.lineAt(range.head);
    const insert = line.text.trim() === "" ? "---\n" : "\n---\n";
    view.dispatch({
      changes: { from: range.head, insert },
      selection: EditorSelection.cursor(range.head + insert.length),
    });
    view.focus();
    return;
  }
  if (action.kind === "fence") {
    const text = state.sliceDoc(range.from, range.to);
    const insert = text === "" ? "```\n\n```" : `\`\`\`\n${text}\n\`\`\``;
    view.dispatch({
      changes: { from: range.from, to: range.to, insert },
      selection: EditorSelection.cursor(range.from + 4),
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
  const last = state.doc.lineAt(range.to);
  const changes = [];
  for (let n = first.number; n <= last.number; n++) {
    const line = state.doc.line(n);
    const stripped = line.text.replace(STRIP_MARKER, "");
    const had = action.match.test(line.text);
    const insert = `${had && line.text === action.token + stripped ? "" : action.token}${stripped}`;
    changes.push({ from: line.from, to: line.to, insert });
  }
  view.dispatch({ changes });
  view.focus();
}

/** The collapsible formatting row under the editor header. */
export function MarkdownToolbar({
  view,
  open,
  onToggle,
}: {
  /** The live editor view; null while the buffer is not editable. */
  view: EditorView | null;
  open: boolean;
  onToggle: () => void;
}) {
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
    <div className="flex h-8 shrink-0 items-center gap-0.5 overflow-x-auto border-b px-2" data-testid="markdown-toolbar">
      {open &&
        groups.map((group, index) => (
          <span key={index} className="flex items-center gap-0.5">
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
      <span className="flex-1" />
      <Tooltip content={t("toolbarToggle")}>
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("toolbarToggle")}
          aria-expanded={open}
          data-testid="markdown-toolbar-toggle"
          onClick={onToggle}
        >
          {open ? <ChevronUp className="size-4 stroke-[1.5]" /> : <ChevronDown className="size-4 stroke-[1.5]" />}
        </Button>
      </Tooltip>
    </div>
  );
}
