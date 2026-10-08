import { useCallback, useEffect, useRef, useState } from "react";
import { Compartment, EditorState as CodeMirrorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  highlightSelectionMatches,
  openSearchPanel,
  search,
  searchKeymap,
} from "@codemirror/search";
import { MergeView, unifiedMergeView } from "@codemirror/merge";
import { CopyPlus, FileDiff, GitMerge, Save, Search, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { api, ApiError } from "../api/client";
import { Button } from "./ui/button";
import { SaveIndicator, type SaveTone } from "./SaveIndicator";
import { Tooltip } from "./ui/tooltip";
import { cn } from "../lib/utils";

/**
 * Editor state machine (design §13.2):
 * loading → clean → dirty → saving → clean | conflict | error.
 * One save in flight; keystrokes during a save buffer and re-dirty after.
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

function baseExtensions(onDocChange: (text: string) => void) {
  return [
    markdown({ base: markdownLanguage }),
    EditorView.lineWrapping,
    search({ top: true }),
    highlightSelectionMatches(),
    keymap.of(searchKeymap),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) onDocChange(update.state.doc.toString());
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
}: {
  state: EditorState;
  onChange: (next: EditorState) => void;
  onStateChange: (next: EditorState) => void;
  /** Session expired mid-edit: the workspace logs out and shows a banner. */
  onAuthLost?: () => void;
  /** Ask the workspace to trash the open file (confirm dialog lives there). */
  onTrashCurrent?: () => void;
  canTrash?: boolean;
}) {
  const { t } = useTranslation("editor");
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const stateRef = useRef<EditorState>(state);
  stateRef.current = state;
  const [diffOpen, setDiffOpen] = useState(false);
  // Swap the inline diff extension without rebuilding the editor.
  const diffCompartment = useRef(new Compartment());

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
    const view = new EditorView({
      parent: hostRef.current,
      state: CodeMirrorState.create({
        doc: currentDoc(stateRef.current),
        extensions: [
          ...baseExtensions(onDocChange),
          // The inline diff extension is swapped in/out via this compartment;
          // the doc listener above runs in every state (saves buffer typing).
          diffCompartment.current.of([]),
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
    if (state.kind === "conflict") setDiffOpen(false);

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

  // Ctrl+S saves; P0 keeps saving explicit (design §13.2).
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

  const createSave = async () => {
    const current = stateRef.current;
    if (current.kind !== "clean" && current.kind !== "dirty") return;
    const path = window.prompt(t("saveAsPrompt"), current.path);
    if (!path) return;
    try {
      const result = await api.saveText(path, current.text, { mode: "create" });
      onStateChange(freshEditorState(result.path, current.text, result.revision));
    } catch (error) {
      window.alert(error instanceof Error ? error.message : String(error));
    }
  };

  const openFind = () => {
    const view = viewRef.current;
    if (view && state.kind !== "conflict") openSearchPanel(view);
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

  return (
    <div
      className="flex h-full min-h-0 flex-col overflow-hidden rounded-lg border bg-card shadow-card"
      data-testid="editor-pane"
    >
      <div className="flex h-11 min-h-11 shrink-0 items-center gap-1 border-b bg-card px-2.5">
        <span
          className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold text-foreground"
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
            <Tooltip content={t("searchTooltip")}>
              <Button variant="ghost" size="icon" onClick={openFind}>
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
            <Tooltip content={t("saveTooltip")}>
              <Button
                size="sm"
                className="ml-1"
                disabled={state.kind !== "dirty" && state.kind !== "error"}
                onClick={() => void save()}
              >
                <Save />
                {t("save")}
              </Button>
            </Tooltip>
            {state.kind === "clean" || state.kind === "dirty" ? (
              <Tooltip content={t("saveAs")}>
                <Button variant="ghost" size="icon" onClick={() => void createSave()}>
                  <CopyPlus />
                </Button>
              </Tooltip>
            ) : null}
            {canTrash ? (
              <Tooltip content={t("trashTooltip")}>
                <Button
                  variant="dangerGhost"
                  size="icon"
                  data-testid="trash-current"
                  onClick={onTrashCurrent}
                >
                  <Trash2 />
                </Button>
              </Tooltip>
            ) : null}
          </div>
        )}
      </div>

      <div className="relative min-h-0 flex-1">
        {inConflict && <ConflictView state={state} stateRef={stateRef} onChange={onChange} />}
        <div
          className={`absolute inset-0 overflow-auto py-2${inConflict ? " invisible" : ""}`}
          data-testid="codemirror-host"
          ref={hostRef}
        />
        {state.kind === "empty" && (
          <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-2 text-muted-foreground">
            <svg
              aria-hidden
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              className="size-8 opacity-60"
            >
              <path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15.5H6.5A2.5 2.5 0 0 0 4 21z" />
              <path d="M4 5.5v13" />
              <path d="M9 8h7M9 11.5h5" strokeLinecap="round" />
            </svg>
            <p className="text-xs">{t("emptyState")}</p>
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center justify-between border-t bg-muted/20 px-2.5 py-1 text-[11px] text-muted-foreground">
        <span className="font-mono">
          {t("stats", {
            chars: stats.chars,
            words: stats.words,
            lines: stats.lines,
          })}
        </span>
        <span className="font-mono">
          {inConflict || state.kind === "empty" || state.kind === "loading"
            ? null
            : t("revLabel", { revision: state.baseRevision.slice(0, 8) })}
        </span>
      </div>
    </div>
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
}: {
  state: Extract<EditorState, { kind: "conflict" }>;
  stateRef: React.RefObject<EditorState>;
  onChange: (next: EditorState) => void;
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
            ...baseExtensions(onDocChange),
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
        extensions: baseExtensions(onDocChange),
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
        <GitMerge className="mt-0.5 size-4 shrink-0" style={{ color: "var(--warning)" }} />
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
        <Button data-testid="conflict-keep-local" onClick={() => void saveWithRemoteBase()}>
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
        "rounded px-2 py-1 text-[10.5px] font-medium transition-colors",
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
