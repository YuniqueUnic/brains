import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Compartment, EditorState as CodeMirrorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import {
  defaultKeymap,
  history,
  historyKeymap,
  redo,
  redoDepth,
  undo,
  undoDepth,
} from "@codemirror/commands";
import {
  highlightSelectionMatches,
  openSearchPanel,
  search,
  searchKeymap,
} from "@codemirror/search";
import { MergeView, unifiedMergeView } from "@codemirror/merge";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { createFindPanel, type FindPanelLabels } from "./editor/find-panel";
import { Toggle } from "@base-ui-components/react/toggle";
import { ToggleGroup } from "@base-ui-components/react/toggle-group";
import { Switch } from "@base-ui-components/react/switch";
import { Slider } from "@base-ui-components/react/slider";
import { Popover } from "@base-ui-components/react/popover";
import { Settings2, RotateCcw } from "lucide-react";
import {
  CopyPlus,
  Eye,
  FileDiff,
  GitMerge,
  Info,
  MoreHorizontal,
  Redo2,
  Save,
  Search,
  Sparkles,
  SquarePen,
  Trash2,
  Undo2,
} from "lucide-react";
import { Menu } from "@base-ui-components/react/menu";
import { useTranslation } from "react-i18next";
import { api, ApiError } from "../api/client";
import { toast } from "sonner";
import { Button } from "./ui/button";
import { SaveIndicator, type SaveTone } from "./SaveIndicator";
import { Tooltip } from "./ui/tooltip";
import { menuItemClass, menuPopupClass } from "./ui/menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { Input } from "./ui/input";
import { cn } from "../lib/utils";

/*
 * Editor (DESIGN.md §26-34): the document is the star — 860px measure (CSS),
 * document typography, restrained markdown highlighting. Chrome is one quiet
 * toolbar: undo/redo, find, diff, save; everything else lives in ⋯.
 * State machine unchanged (design §13.2): loading → clean → dirty → saving
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

function freshEditorState(path: string, text: string, baseRevision: string): EditorState {
  return { kind: "clean", path, text, baseText: text, baseRevision };
}

/* Editor preferences (fonts are independent of the UI font): persisted,
 * applied through compartments so the view never rebuilds. */
export interface EditorSettings {
  lineNumbers: boolean;
  wrap: boolean;
  font: "sans" | "serif" | "mono";
  /** Percent, 100 = the 15px document base. */
  scale: number;
}

const EDITOR_DEFAULTS: EditorSettings = { lineNumbers: false, wrap: true, font: "sans", scale: 100 };
const EDITOR_KEY = "brains-editor";

function loadEditorSettings(): EditorSettings {
  try {
    const raw = localStorage.getItem(EDITOR_KEY);
    if (raw) return { ...EDITOR_DEFAULTS, ...(JSON.parse(raw) as Partial<EditorSettings>) };
  } catch {
    /* ignore malformed state */
  }
  return EDITOR_DEFAULTS;
}

function saveEditorSettings(settings: EditorSettings) {
  try {
    localStorage.setItem(EDITOR_KEY, JSON.stringify(settings));
  } catch {
    /* best-effort */
  }
}

/* Markdown highlighting (DESIGN.md §31): near-monochrome, quiet accents. */
const markdownHighlights = HighlightStyle.define([
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

function findPanelLabels(t: (key: string) => string): FindPanelLabels {
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

function baseExtensions(
  onDocChange: (text: string) => void,
  onHistoryChange: (canUndo: boolean, canRedo: boolean) => void,
  findLabels: () => FindPanelLabels,
) {
  return [
    markdown({ base: markdownLanguage }),
    syntaxHighlighting(markdownHighlights),
    EditorView.lineWrapping,
    history(),
    search({ top: true, createPanel: (view) => createFindPanel(findLabels())(view) }),
    highlightSelectionMatches(),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) onDocChange(update.state.doc.toString());
      onHistoryChange?.(undoDepth(update.state) > 0, redoDepth(update.state) > 0);
    }),
  ];
}

export default function EditorPane({
  state,
  onChange,
  onStateChange,
  onAuthLost,
  onTrashCurrent,
  canTrash,
  onOpenProperties,
}: {
  state: EditorState;
  onChange: (next: EditorState) => void;
  onStateChange: (next: EditorState) => void;
  /** Session expired mid-edit: the workspace logs out and shows a banner. */
  onAuthLost?: () => void;
  /** Ask the workspace to trash the open file (confirm dialog lives there). */
  onTrashCurrent?: () => void;
  canTrash?: boolean;
  /** Open the properties panel for the active document (toolbar button). */
  onOpenProperties?: () => void;
}) {
  const { t } = useTranslation("editor");
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const stateRef = useRef<EditorState>(state);
  stateRef.current = state;
  const [diffOpen, setDiffOpen] = useState(false);
  const [viewMode, setViewMode] = useState<"source" | "preview">("source");
  const [undoPossible, setUndoPossible] = useState(false);
  const [redoPossible, setRedoPossible] = useState(false);
  const [settings, setSettings] = useState<EditorSettings>(loadEditorSettings);
  const updateSettings = (patch: Partial<EditorSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      saveEditorSettings(next);
      return next;
    });
  };
  // Swap the inline diff extension without rebuilding the editor.
  const diffCompartment = useRef(new Compartment());
  const lineNumComp = useRef(new Compartment());
  const wrapComp = useRef(new Compartment());
  const sizeComp = useRef(new Compartment());

  useEffect(() => {
    if (!hostRef.current) return;
    const onDocChange = (text: string) => {
      const current = stateRef.current;
      if (current.kind === "clean" || current.kind === "dirty" || current.kind === "error") {
        const next = {
          ...current,
          kind: text === current.baseText ? ("clean" as const) : ("dirty" as const),
          text,
          message: undefined,
        };
        // Sync the ref BEFORE notifying React: Ctrl+S must see the
        // freshest buffer even if the keystroke and the hotkey land in
        // the same frame, before the re-render catches up.
        stateRef.current = next;
        onChange(next);
      } else if (current.kind === "saving") {
        // Buffer keystrokes; the save-completion handler re-evaluates.
        current.text = text;
      }
    };
    // Push history depth to React state only when the affordance flips, so
    // keystrokes don't re-render the pane per character.
    const onHistoryChange = (canUndo: boolean, canRedo: boolean) => {
      setUndoPossible((prev) => (prev === canUndo ? prev : canUndo));
      setRedoPossible((prev) => (prev === canRedo ? prev : canRedo));
    };
    const initialSettings = loadEditorSettings();
    const view = new EditorView({
      parent: hostRef.current,
      state: CodeMirrorState.create({
        doc: currentDoc(stateRef.current),
        extensions: [
          ...baseExtensions(onDocChange, onHistoryChange, () => findPanelLabels(t)),
          // The inline diff extension is swapped in/out via this compartment;
          // the doc listener above runs in every state (saves buffer typing).
          diffCompartment.current.of([]),
          lineNumComp.current.of(initialSettings.lineNumbers ? lineNumbers() : []),
          wrapComp.current.of(initialSettings.wrap ? EditorView.lineWrapping : []),
          sizeComp.current.of(
            EditorView.theme({
              "&": { fontSize: `${Math.round(15 * initialSettings.scale) / 100}px` },
            }),
          ),
        ],
      }),
    });
    viewRef.current = view;
    return () => view.destroy();
    // Editor instance lives across file switches; doc is swapped below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Inline diff against the saved base (VSCode-style): chunks highlight the
  // unsaved changes; per-chunk revert restores the saved text. Exits on save.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const canDiff =
      diffOpen &&
      (state.kind === "dirty" || state.kind === "error") &&
      state.text !== state.baseText;
    if (canDiff) {
      view.dispatch({
        effects: diffCompartment.current.reconfigure(
          unifiedMergeView({
            original: state.baseText,
            mergeControls: true,
            highlightChanges: true,
            gutter: true,
            allowInlineDiffs: true,
            collapseUnchanged: { margin: 3, minSize: 8 },
          }),
        ),
      });
    } else {
      view.dispatch({ effects: diffCompartment.current.reconfigure([]) });
      if (diffOpen && state.kind === "clean") setDiffOpen(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [diffOpen, state]);

  // Doc resync happens ONLY when the buffer's identity (docKey: file switch,
  // conflict, restore) changes — never per keystroke. CodeMirror is the
  // source of truth for the visible buffer within one identity.
  const lastDocRef = useRef<{ key: string; state: EditorState }>({ key: docKey(state), state });
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const key = docKey(state);
    const previous = lastDocRef.current;
    if (key === previous.key) return;
    lastDocRef.current = { key, state };
    if (state.kind === "conflict") {
      setDiffOpen(false);
      setViewMode("source");
    }

    // Text typed while NO file was bound to the editor (the 新建 dialog is
    // still committing) belongs to the user, not to the state replacing it:
    // adopt it onto the freshly created base instead of wiping it. Loading
    // transitions resync normally — that buffer belongs to another file.
    const viewText = view.state.doc.toString();
    const typedBlind = previous.state.kind === "empty";
    if (
      typedBlind &&
      viewText !== "" &&
      (state.kind === "clean" || state.kind === "dirty" || state.kind === "error")
    ) {
      const adopted: EditorState = {
        ...state,
        text: viewText,
        kind: viewText === state.baseText ? "clean" : "dirty",
      };
      stateRef.current = adopted;
      onChange(adopted);
      return;
    }
    const doc = currentDoc(state);
    if (view.state.doc.toString() !== doc) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: doc } });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  // Apply preference changes through compartments: no view rebuild, no
  // buffer churn.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: [
        lineNumComp.current.reconfigure(settings.lineNumbers ? lineNumbers() : []),
        wrapComp.current.reconfigure(settings.wrap ? EditorView.lineWrapping : []),
        sizeComp.current.reconfigure(
          EditorView.theme({ "&": { fontSize: `${Math.round(15 * settings.scale) / 100}px` } }),
        ),
      ],
    });
  }, [settings.lineNumbers, settings.wrap, settings.scale]);

  const save = useCallback(async () => {
    const current = stateRef.current;
    // `error` keeps the last text+precondition: retrying must be possible
    // without retyping (design §13.2 — errors are states, not dead ends).
    if (current.kind !== "dirty" && current.kind !== "error") return;
    const operationId = crypto.getRandomValues(new Uint8Array(16)).reduce(
      (acc, b) => acc + b.toString(16).padStart(2, "0"),
      "",
    );
    const saving: EditorState = {
      kind: "saving",
      path: current.path,
      text: current.text,
      baseText: current.baseText,
      baseRevision: current.baseRevision,
      operationId,
    };
    onStateChange(saving);
    stateRef.current = saving;
    // True while this save still owns the editor: opening another file
    // replaces the state and must not be clobbered by this completion.
    const stillMine = () =>
      stateRef.current.kind === "saving" &&
      stateRef.current.path === current.path &&
      stateRef.current.operationId === operationId;
    try {
      const result = await api.saveText(current.path, current.text, {
        mode: "update",
        baseRevision: current.baseRevision,
      }, operationId);
      if (!stillMine()) return;
      const buffered = stateRef.current.kind === "saving" ? stateRef.current.text : current.text;
      // The server holds `current.text` (what this request carried); anything
      // typed during the flight stays on top of THAT base — never its own.
      const next =
        buffered === current.text
          ? freshEditorState(current.path, buffered, result.revision)
          : {
              kind: "dirty" as const,
              path: current.path,
              text: buffered,
              baseText: current.text,
              baseRevision: result.revision,
            };
      onStateChange(next);
      stateRef.current = next;
    } catch (error) {
      if (!stillMine()) return;
      // An expired session is not a retryable editor error — hand it up so
      // the user is logged out instead of stuck in error → retry → 401.
      if (error instanceof ApiError && error.status === 401) {
        onAuthLost?.();
        return;
      }
      // Keystrokes typed during the failed flight live in the saving state's
      // text: they are on screen and must survive into the next state, or a
      // retry would silently erase them (design §13.2).
      const buffered = stateRef.current.kind === "saving" ? stateRef.current.text : current.text;
      if (error instanceof ApiError && error.status === 412) {
        let remoteText = "";
        try {
          remoteText = (await api.readText(current.path)).text;
        } catch {
          remoteText = t("conflictRemotePlaceholder");
        }
        if (!stillMine()) return;
        const conflict: EditorState = {
          kind: "conflict",
          path: current.path,
          localText: buffered,
          baseText: current.baseText,
          remoteRevision: error.currentRevision ?? "(unknown)",
          remoteText,
        };
        onStateChange(conflict);
        stateRef.current = conflict;
        return;
      }
      if (current.kind === "dirty" || current.kind === "error") {
        const failed: EditorState = {
          ...current,
          text: buffered,
          kind: "error",
          message: error instanceof Error ? error.message : String(error),
        };
        onStateChange(failed);
        stateRef.current = failed;
      }
    }
  }, [onStateChange, onAuthLost, t]);

  // Ctrl+S saves; saving stays explicit (design §13.2).
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [save]);

  const [saveAsOpen, setSaveAsOpen] = useState(false);
  const [saveAsPath, setSaveAsPath] = useState("");
  const openSaveAs = () => {
    const current = stateRef.current;
    if (current.kind !== "clean" && current.kind !== "dirty") return;
    setSaveAsPath(current.path);
    setSaveAsOpen(true);
  };
  const commitSaveAs = async () => {
    const current = stateRef.current;
    const path = saveAsPath.trim();
    if (current.kind !== "clean" && current.kind !== "dirty") return;
    if (!path) return;
    try {
      const result = await api.saveText(path, current.text, { mode: "create" });
      setSaveAsOpen(false);
      onStateChange(freshEditorState(result.path, current.text, result.revision));
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        onAuthLost?.();
        return;
      }
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const openFind = () => {
    const view = viewRef.current;
    if (view && state.kind !== "conflict") openSearchPanel(view);
  };

  const runUndo = () => {
    const view = viewRef.current;
    if (view) undo(view);
  };
  const runRedo = () => {
    const view = viewRef.current;
    if (view) redo(view);
  };

  // The CodeMirror host must stay mounted through EVERY state — including
  // conflict, which renders its panel as a sibling and hides (never unmounts)
  // the editor. Unmounting would orphan the one EditorView created on mount
  // and leave a dead editor after conflict resolution.
  const inConflict = state.kind === "conflict";
  const buffer = currentDoc(state);
  const stats = {
    chars: buffer.length,
    words: buffer.split(/\s+/).filter(Boolean).length,
    lines: buffer === "" ? 1 : buffer.split("\n").length,
  };
  const canDiff =
    (state.kind === "dirty" || state.kind === "error") && state.text !== state.baseText;
  const previewHtml = useMemo(() => {
    if (viewMode !== "preview" || buffer === "") return "";
    return DOMPurify.sanitize(marked.parse(buffer, { async: false }) as string);
  }, [viewMode, buffer]);

  return (
    <div
      className="flex h-full min-h-0 flex-col bg-card"
      data-testid="editor-pane"
    >
      <div className="flex h-11 shrink-0 items-center gap-1 border-b px-2.5">
        <span
          className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground"
          data-testid="editor-path"
          title={state.kind === "empty" ? undefined : state.path}
        >
          {state.kind === "empty" ? "" : state.path}
        </span>
        <SaveIndicator
          tone={statusTone(state)}
          label={statusLabel(state, t)}
          busy={state.kind === "saving" || state.kind === "loading"}
          title={state.kind === "error" ? state.message : undefined}
        />
        {inConflict || state.kind === "empty" ? null : (
          <div className="flex items-center gap-0.5">
            <ToggleGroup
              value={[viewMode]}
              onValueChange={(groupValue) =>
                setViewMode((groupValue[groupValue.length - 1] ?? "source") as "source" | "preview")
              }
              className="mr-1 flex items-center gap-0.5 rounded-[7px] border p-0.5"
            >
              <Toggle
                value="source"
                aria-label={t("modeSource")}
                className="flex h-6 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
              >
                <SquarePen className="size-3.5 stroke-[1.5]" />
                {t("modeSource")}
              </Toggle>
              <Toggle
                value="preview"
                aria-label={t("modePreview")}
                className="flex h-6 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
              >
                <Eye className="size-3.5 stroke-[1.5]" />
                {t("modePreview")}
              </Toggle>
            </ToggleGroup>
            {onOpenProperties && (
              <Tooltip content={`${t("properties")}（⌘I）`}>
                <Button size="icon" aria-label={t("properties")} onClick={onOpenProperties}>
                  <Info />
                </Button>
              </Tooltip>
            )}
            {viewMode === "source" && (
              <>
            <Tooltip content={t("undoTooltip")}>
              <Button
                size="icon"
                aria-label={t("undoTooltip")}
                disabled={!undoPossible}
                onClick={runUndo}
              >
                <Undo2 />
              </Button>
            </Tooltip>
            <Tooltip content={t("redoTooltip")}>
              <Button
                size="icon"
                aria-label={t("redoTooltip")}
                disabled={!redoPossible}
                onClick={runRedo}
              >
                <Redo2 />
              </Button>
            </Tooltip>
            <span className="mx-1 h-4 w-px bg-border" />
            <Tooltip content={t("searchTooltip")}>
              <Button size="icon" aria-label={t("searchTooltip")} onClick={openFind}>
                <Search />
              </Button>
            </Tooltip>
            <Tooltip content={t("diffTooltip")}>
              <Button
                variant={diffOpen ? "secondary" : "ghost"}
                size="icon"
                aria-pressed={diffOpen}
                disabled={!canDiff && !diffOpen}
                onClick={() => setDiffOpen((v) => !v)}
              >
                <FileDiff />
              </Button>
            </Tooltip>
            </>
            )}
            <Button
              variant="secondary"
              data-testid="save-button"
              className="ml-1"
              disabled={state.kind !== "dirty" && state.kind !== "error"}
              onClick={() => void save()}
            >
              <Save />
              {t("save")}
            </Button>
            <Menu.Root>
              <Menu.Trigger
                render={
                  <Button size="icon" aria-label={t("editorMenu")} data-testid="editor-menu" />
                }
              >
                <MoreHorizontal />
              </Menu.Trigger>
              <Menu.Portal>
                <Menu.Positioner sideOffset={4} align="end">
                  <Menu.Popup className={menuPopupClass}>
                    {state.kind === "clean" || state.kind === "dirty" ? (
                      <Menu.Item className={menuItemClass()} onClick={openSaveAs}>
                        <CopyPlus className="size-4 stroke-[1.5] text-muted-foreground" />
                        {t("saveAs")}
                      </Menu.Item>
                    ) : null}
                    {canTrash ? (
                      <Menu.Item
                        className={menuItemClass({ danger: true })}
                        onClick={onTrashCurrent}
                        data-testid="trash-current"
                      >
                        <Trash2 className="size-4 stroke-[1.5]" />
                        {t("trashTooltip")}
                      </Menu.Item>
                    ) : null}
                  </Menu.Popup>
                </Menu.Positioner>
              </Menu.Portal>
            </Menu.Root>
            <EditorSettingsPopover settings={settings} onChange={updateSettings} />
          </div>
        )}
      </div>

      <div className="relative min-h-0 flex-1" data-editor-font={settings.font}>
        {inConflict && (
          <ConflictView
            state={state}
            stateRef={stateRef}
            onChange={onChange}
            onAuthLost={onAuthLost}
          />
        )}
        <div
          className={`absolute inset-0 overflow-auto${inConflict || viewMode === "preview" ? " invisible" : ""}`}
          data-testid="codemirror-host"
          ref={hostRef}
        />
        {!inConflict && viewMode === "preview" && (
          <div
            className="markdown-preview absolute inset-0 overflow-auto"
            data-testid="markdown-preview"
            dangerouslySetInnerHTML={{ __html: previewHtml }}
          />
        )}
        {state.kind === "empty" && (
          <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-3 text-muted-foreground">
            <Sparkles className="size-5 stroke-[1.25] opacity-50" />
            <div className="text-center">
              <p className="text-[13px] text-foreground/80">{t("emptyTitle")}</p>
              <p className="mt-1 text-xs">{t("emptyState")}</p>
            </div>
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-3 border-t px-3 py-1.5 text-[11px] text-faint">
        <span className="font-mono">
          {t("stats", {
            chars: stats.chars,
            words: stats.words,
            lines: stats.lines,
          })}
        </span>
        <span className="flex-1" />
        {inConflict || state.kind === "empty" ? null : (
          <div className="flex items-center gap-2">
            <Slider.Root
              value={settings.scale}
              onValueChange={(value) => updateSettings({ scale: typeof value === "number" ? value : (value[0] ?? settings.scale) })}
              min={80}
              max={160}
              step={5}
              aria-label={t("scaleLabel")}
              className="flex h-4 w-24 items-center"
            >
              <Slider.Control className="flex h-4 w-full items-center rounded">
                <Slider.Track className="h-1 w-full rounded-full bg-muted">
                  <Slider.Indicator className="rounded-full bg-primary/60" />
                </Slider.Track>
                <Slider.Thumb className="size-3 cursor-pointer rounded-full border border-border bg-card shadow-menu" />
              </Slider.Control>
            </Slider.Root>
            <button
              type="button"
              aria-label={t("scaleReset")}
              title={t("scaleReset")}
              className="cursor-pointer rounded p-0.5 hover:bg-muted hover:text-foreground"
              onClick={() => updateSettings({ scale: 100 })}
            >
              <RotateCcw className="size-3.5 stroke-[1.5]" />
            </button>
            <ToggleGroup
              value={[settings.wrap ? "wrap" : "overflow"]}
              onValueChange={(groupValue) =>
                updateSettings({ wrap: (groupValue[groupValue.length - 1] ?? "wrap") === "wrap" })
              }
              className="flex items-center gap-0.5 rounded-md border p-0.5"
            >
              <Toggle
                value="wrap"
                aria-label={t("wrapLabel")}
                className="cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
              >
                {t("wrapLabel")}
              </Toggle>
              <Toggle
                value="overflow"
                aria-label={t("overflowLabel")}
                className="cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
              >
                {t("overflowLabel")}
              </Toggle>
            </ToggleGroup>
          </div>
        )}
        <span className="font-mono">
          {inConflict || state.kind === "empty" || state.kind === "loading"
            ? null
            : t("revLabel", { revision: state.baseRevision.slice(0, 8) })}
        </span>
      </div>

      <Dialog open={saveAsOpen} onOpenChange={setSaveAsOpen}>
        <DialogContent data-testid="save-as-dialog">
          <DialogHeader>
            <DialogTitle>{t("saveAs")}</DialogTitle>
            <DialogDescription>{t("saveAsPrompt")}</DialogDescription>
          </DialogHeader>
          <div className="pt-4">
            <Input
              data-testid="save-as-path"
              value={saveAsPath}
              onChange={(event) => setSaveAsPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void commitSaveAs();
              }}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setSaveAsOpen(false)}>
              {t("close")}
            </Button>
            <Button variant="default" data-testid="save-as-confirm" onClick={() => void commitSaveAs()}>
              {t("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/**
 * Editor preferences popover (line numbers, wrap, font family, scale).
 * The editor font is independent of the UI font.
 */
function EditorSettingsPopover({
  settings,
  onChange,
}: {
  settings: EditorSettings;
  onChange: (patch: Partial<EditorSettings>) => void;
}) {
  const { t } = useTranslation("editor");
  return (
    <Popover.Root>
      <Popover.Trigger
        render={<Button size="icon" aria-label={t("settingsLabel")} data-testid="editor-settings" />}
      >
        <Settings2 />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner align="end" sideOffset={6}>
          <Popover.Popup className="z-50 w-[248px] rounded-[10px] border bg-card p-3 text-[12.5px] shadow-menu overlay-in">
            <div className="flex items-center justify-between py-1.5">
              <span>{t("lineNumbersLabel")}</span>
              <Switch.Root
                checked={settings.lineNumbers}
                onCheckedChange={(checked) => onChange({ lineNumbers: checked === true })}
                className="relative h-5 w-8 cursor-pointer rounded-full bg-muted transition-colors data-[checked]:bg-primary"
              >
                <Switch.Thumb className="absolute left-0.5 top-0.5 size-4 rounded-full bg-card shadow transition-transform data-[checked]:translate-x-3" />
              </Switch.Root>
            </div>
            <div className="flex items-center justify-between py-1.5">
              <span>{t("wrapLabel")}</span>
              <Switch.Root
                checked={settings.wrap}
                onCheckedChange={(checked) => onChange({ wrap: checked === true })}
                className="relative h-5 w-8 cursor-pointer rounded-full bg-muted transition-colors data-[checked]:bg-primary"
              >
                <Switch.Thumb className="absolute left-0.5 top-0.5 size-4 rounded-full bg-card shadow transition-transform data-[checked]:translate-x-3" />
              </Switch.Root>
            </div>
            <div className="flex items-center justify-between py-1.5">
              <span>{t("fontLabel")}</span>
              <ToggleGroup
                value={[settings.font]}
                onValueChange={(groupValue) =>
                  onChange({ font: (groupValue[groupValue.length - 1] ?? "sans") as EditorSettings["font"] })
                }
                className="flex items-center gap-0.5 rounded-md border p-0.5"
              >
                {(
                  [
                    ["sans", t("fontSans")],
                    ["serif", t("fontSerif")],
                    ["mono", t("fontMono")],
                  ] as const
                ).map(([value, label]) => (
                  <Toggle
                    key={value}
                    value={value}
                    aria-label={label}
                    className="cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
                  >
                    {label}
                  </Toggle>
                ))}
              </ToggleGroup>
            </div>
            <div className="flex items-center gap-2 py-1.5">
              <span className="shrink-0">{t("scaleLabel")}</span>
              <Slider.Root
                value={settings.scale}
                onValueChange={(value) =>
                  onChange({ scale: typeof value === "number" ? value : (value[0] ?? settings.scale) })
                }
                min={80}
                max={160}
                step={5}
                aria-label={t("scaleLabel")}
                className="flex h-4 flex-1 items-center"
              >
                <Slider.Control className="flex h-4 w-full items-center rounded">
                  <Slider.Track className="h-1 w-full rounded-full bg-muted">
                    <Slider.Indicator className="rounded-full bg-primary/60" />
                  </Slider.Track>
                  <Slider.Thumb className="size-3 cursor-pointer rounded-full border border-border bg-card shadow-menu" />
                </Slider.Control>
              </Slider.Root>
              <span className="w-10 shrink-0 text-right font-mono text-[11px]">{settings.scale}%</span>
              <button
                type="button"
                aria-label={t("scaleReset")}
                className="cursor-pointer rounded p-0.5 hover:bg-muted hover:text-foreground"
                onClick={() => onChange({ scale: 100 })}
              >
                <RotateCcw className="size-3.5 stroke-[1.5]" />
              </button>
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

type ConflictMode = "split" | "inline";

/**
 * Conflict resolution with VSCode-style diff views: "split" shows remote and
 * local side-by-side with synced chunk highlights; "inline" shows the local
 * buffer against the remote original with per-chunk accept/reject. The merge
 * editor owns the buffer while the conflict is open — text typed here flows
 * back into `localText` through the same state machine.
 */
function ConflictView({
  state,
  stateRef,
  onChange,
  onAuthLost,
}: {
  state: Extract<EditorState, { kind: "conflict" }>;
  stateRef: React.RefObject<EditorState>;
  onChange: (next: EditorState) => void;
  onAuthLost?: () => void;
}) {
  const { t } = useTranslation("editor");
  const [mode, setMode] = useState<ConflictMode>("split");
  const hostRef = useRef<HTMLDivElement | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  // True while THIS conflict still owns the editor (the human may have
  // opened another file while the request was in flight).
  const stillMine = () =>
    stateRef.current.kind === "conflict" &&
    stateRef.current.path === state.path &&
    stateRef.current.remoteRevision === state.remoteRevision;

  useEffect(() => {
    const parent = hostRef.current;
    if (!parent) return;
    const onDocChange = (text: string) => {
      const current = stateRef.current;
      if (
        current.kind === "conflict" &&
        current.path === state.path &&
        current.remoteRevision === state.remoteRevision &&
        current.localText !== text
      ) {
        onChangeRef.current({ ...current, localText: text });
      }
    };
    if (mode === "inline") {
      const view = new EditorView({
        parent,
        state: CodeMirrorState.create({
          doc: state.localText,
          extensions: [
            ...baseExtensions(onDocChange, () => undefined, () => findPanelLabels(t)),
            unifiedMergeView({
              original: state.remoteText,
              mergeControls: true,
              highlightChanges: true,
              gutter: true,
              allowInlineDiffs: true,
              collapseUnchanged: { margin: 3, minSize: 8 },
            }),
          ],
        }),
      });
      return () => view.destroy();
    }
    const merge = new MergeView({
      a: {
        doc: state.remoteText,
        extensions: [
          markdown({ base: markdownLanguage }),
          EditorView.lineWrapping,
          EditorView.editable.of(false),
        ],
      },
      b: {
        doc: state.localText,
        extensions: baseExtensions(onDocChange, () => undefined, () => findPanelLabels(t)),
      },
      parent,
      highlightChanges: true,
      gutter: true,
      collapseUnchanged: { margin: 3, minSize: 8 },
    });
    return () => merge.destroy();
    // Rebuild only on mode or conflict identity change: the merge editors
    // own the buffer text in between (prop pushes would clobber typing).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, state.path, state.remoteRevision]);

  const saveWithRemoteBase = async () => {
    const current = stateRef.current;
    const localText = current.kind === "conflict" ? current.localText : state.localText;
    try {
      const result = await api.saveText(state.path, localText, {
        mode: "update",
        baseRevision: state.remoteRevision,
      });
      if (!stillMine()) return;
      onChange(freshEditorState(state.path, localText, result.revision));
    } catch (error) {
      if (!stillMine()) return;
      if (error instanceof ApiError && error.status === 401) {
        onAuthLost?.();
        return;
      }
      if (error instanceof ApiError && error.status === 412) {
        const nextRevision = error.currentRevision ?? state.remoteRevision;
        let remoteText = state.remoteText;
        let notice: string | undefined;
        try {
          remoteText = (await api.readText(state.path)).text;
        } catch {
          // Never pair a NEW revision with the STALE text the human compared:
          // keep the old pair together and say why (design §13.2).
          notice = t("conflictReReadFailed");
        }
        onChange({
          ...state,
          remoteRevision: notice ? state.remoteRevision : nextRevision,
          remoteText,
          notice,
        });
        return;
      }
      onChange({
        ...state,
        notice: error instanceof Error ? error.message : String(error),
      });
    }
  };

  const takeRemote = () => {
    onChange({
      kind: "clean",
      path: state.path,
      text: state.remoteText,
      baseText: state.remoteText,
      baseRevision: state.remoteRevision,
    });
  };

  const remoteUsable = state.remoteRevision !== "(unknown)";

  return (
    <div
      className="absolute inset-0 z-10 flex flex-col gap-1.5 border-b"
      data-testid="conflict-view"
      style={{
        borderColor: "color-mix(in srgb, var(--warning) 30%, var(--border))",
        backgroundColor: "color-mix(in srgb, var(--warning) 6%, var(--card))",
      }}
    >
      <div className="flex items-start gap-2 px-3 pt-2.5">
        <GitMerge className="mt-0.5 size-4 shrink-0 stroke-[1.5]" style={{ color: "var(--warning)" }} />
        <div className="min-w-0 flex-1">
          <p className="text-[12.5px] font-semibold">{t("conflictTitle")}</p>
          <p className="mt-0.5 text-[11.5px] text-muted-foreground">
            {t("conflictDesc", { revision: state.remoteRevision.slice(0, 8) })}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-0.5 rounded-md border bg-card p-0.5">
          <ModeChip active={mode === "split"} onClick={() => setMode("split")}>
            {t("conflictModeSplit")}
          </ModeChip>
          <ModeChip active={mode === "inline"} onClick={() => setMode("inline")}>
            {t("conflictModeInline")}
          </ModeChip>
        </div>
      </div>
      {state.notice && (
        <p
          className="mx-3 rounded-md px-2.5 py-1.5 text-xs"
          role="alert"
          style={{
            backgroundColor: "color-mix(in srgb, var(--destructive) 8%, transparent)",
            color: "color-mix(in srgb, var(--destructive) 80%, var(--foreground))",
          }}
        >
          {state.notice}
        </p>
      )}
      <div className="min-h-0 flex-1 px-3">
        <div
          ref={hostRef}
          className="h-full overflow-hidden rounded-md border bg-card [&_.cm-editor]:h-full"
        />
      </div>
      <div className="flex shrink-0 items-center justify-end gap-2 px-3 py-2">
        <Button variant="secondary" data-testid="conflict-keep-local" onClick={() => void saveWithRemoteBase()}>
          {t("conflictKeepLocal")}
        </Button>
        <Button
          variant="outline"
          data-testid="conflict-take-remote"
          onClick={takeRemote}
          disabled={!remoteUsable}
          title={remoteUsable ? undefined : t("conflictRemoteUnreadable")}
        >
          {t("conflictTakeRemote")}
        </Button>
      </div>
    </div>
  );
}

function ModeChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "cursor-pointer rounded px-2 py-1 text-[10.5px] font-medium transition-colors",
        active
          ? "bg-primary/12 text-primary ring-1 ring-primary/30"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** Traffic-light tone for the save capsule; labels stay the e2e contract. */
function statusTone(state: EditorState): SaveTone {
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

function statusLabel(state: EditorState, t: (key: string) => string): string {
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

function currentDoc(state: EditorState): string {
  if (state.kind === "empty" || state.kind === "loading") return "";
  if (state.kind === "conflict") return state.localText;
  return state.text;
}

function docKey(state: EditorState): string {
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
