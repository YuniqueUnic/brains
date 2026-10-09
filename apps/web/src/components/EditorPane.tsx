import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Compartment, EditorState as CodeMirrorState } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { autocompletion } from "@codemirror/autocomplete";
import { unifiedMergeView } from "@codemirror/merge";
import { openSearchPanel } from "@codemirror/search";
import { redo, undo } from "@codemirror/commands";
import { marked } from "marked";
import DOMPurify from "dompurify";
import {
  CopyPlus,
  Eye,
  FileDiff,
  Info,
  MoreHorizontal,
  Redo2,
  Search,
  Send,
  Sparkles,
  SquarePen,
  Trash2,
  Undo2,
  WrapText,
} from "lucide-react";
import { Menu } from "@base-ui-components/react/menu";
import { Toggle } from "@base-ui-components/react/toggle";
import { useTranslation } from "react-i18next";
import { api, ApiError } from "../api/client";
import { toast } from "sonner";
import { Button } from "./ui/button";
import { SaveIndicator } from "./SaveIndicator";
import { Tooltip } from "./ui/tooltip";
import { menuItemClass, menuPopupClass, MenuShortcut } from "./ui/menu";
import { SliderRow } from "./ui/slider-row";
import { ToggleGroupSection } from "./shell/toggle-section";
import { fileCompletions, type CompletionFile } from "./editor/completions";
import { referenceHover } from "./editor/reference-hover";
import { completionPreview } from "./editor/completion-preview";
import { MarkdownToolbar } from "./editor/markdown-toolbar";
import { ConflictView } from "./editor/conflict-view";
import { SaveAsDialog } from "./editor/save-as-dialog";
import { invalidatePreview } from "./properties/preview";
import { EditorSettingsPopover } from "./editor/editor-settings-popover";
import {
  baseExtensions,
  currentDoc,
  docKey,
  findPanelLabels,
  freshEditorState,
  statusLabel,
  statusTone,
  type EditorState,
} from "./editor/editor-core";
import {
  clearDraft,
  DRAFT_LIMIT,
  flushDrafts,
  loadEditorSettings,
  readDraft,
  saveEditorSettings,
  writeDraft,
  type EditorSettings,
} from "./editor/editor-settings";

/*
 * Editor pane (DESIGN.md §26-34): the document is the star. One quiet
 * toolbar (undo/redo, find, diff, publish), an optional markdown formatting
 * row under it, and the ⋯ menu for the rest. Shared machinery lives in
 * editor/editor-core (state machine, extensions) and editor/editor-settings
 * (preferences, crash-safe drafts); the conflict merge view is
 * editor/conflict-view. Draft/publish model: typing keeps a LOCAL draft;
 * ⌘S stashes it; only 发布 (button / ⌘⏎) writes a new revision.
 */
export default function EditorPane({
  state,
  onChange,
  onStateChange,
  onAuthLost,
  onTrashCurrent,
  canTrash,
  onOpenProperties,
  onOpenReference,
  completions,
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
  /** Open a vault file referenced by hover (docs/… targets, [[wiki]]). */
  onOpenReference?: (path: string) => void;
  /** Vault listing for [[wiki]] / []() completions and hover previews. */
  completions?: CompletionFile[];
}) {
  const { t } = useTranslation("editor");
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const stateRef = useRef<EditorState>(state);
  stateRef.current = state;
  const [viewInstance, setViewInstance] = useState<EditorView | null>(null);
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
  const completionsComp = useRef(new Compartment());
  const previewLabels = { loading: t("previewLoading"), unavailable: t("previewUnavailable") };

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
        if (next.kind === "dirty") writeDraft(current.path, text);
        else clearDraft(current.path);
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
    const completionExtensions = completions
      ? [
          autocompletion({ override: [fileCompletions(completions)] }),
          referenceHover(completions, onOpenReference, previewLabels),
          completionPreview(onOpenReference, previewLabels),
        ]
      : [];
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
          completionsComp.current.of(completionExtensions),
        ],
      }),
    });
    viewRef.current = view;
    setViewInstance(view);
    // Unsaved buffers survive tab close and crashes through their drafts.
    const flush = () => flushDrafts();
    window.addEventListener("beforeunload", flush);
    document.addEventListener("visibilitychange", flush);
    return () => {
      window.removeEventListener("beforeunload", flush);
      document.removeEventListener("visibilitychange", flush);
      view.destroy();
    };
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
    // still committing, or a slow file is still loading) belongs to the
    // user, not to the state replacing it: adopt it onto the freshly created
    // base instead of wiping it.
    const viewText = view.state.doc.toString();
    const typedBlind =
      previous.state.kind === "empty" || previous.state.kind === "loading";
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
    // Crash/refresh safety: a clean buffer adopts a stored draft as dirty
    // content (with a discard toast); a clean buffer without one drops any
    // stale draft — save/revert elsewhere already made it obsolete.
    if (state.kind === "clean") {
      const draft = readDraft(state.path);
      if (draft && draft.text !== state.text && draft.text.length <= DRAFT_LIMIT) {
        const adopted: EditorState = { ...state, text: draft.text, kind: "dirty" };
        stateRef.current = adopted;
        onChange(adopted);
        const rebinding = viewRef.current;
        if (rebinding) {
          rebinding.dispatch({
            changes: { from: 0, to: rebinding.state.doc.length, insert: draft.text },
          });
        }
        const minutes = Math.max(0, Math.floor((Date.now() - draft.at) / 60_000));
        const when =
          minutes < 1
            ? t("draftJustNow")
            : minutes < 60
              ? t("draftMinAgo", { n: minutes })
              : t("draftHourAgo", { n: Math.floor(minutes / 60) });
        toast.info(t("draftRestored", { when }), {
          action: {
            label: t("draftDrop"),
            onClick: () => {
              clearDraft(state.path);
              const cur = stateRef.current;
              if (
                cur.kind === "dirty" &&
                cur.path === state.path &&
                cur.text === draft.text
              ) {
                const reverted: EditorState = {
                  kind: "clean",
                  path: state.path,
                  text: state.text,
                  baseText: state.baseText,
                  baseRevision: state.baseRevision,
                };
                stateRef.current = reverted;
                onChange(reverted);
                const reverting = viewRef.current;
                if (reverting) {
                  reverting.dispatch({
                    changes: {
                      from: 0,
                      to: reverting.state.doc.length,
                      insert: state.text,
                    },
                  });
                }
              }
            },
          },
        });
        return;
      }
      clearDraft(state.path);
    }
    const doc = currentDoc(state);
    if (view.state.doc.toString() !== doc) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: doc } });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  // Apply preference changes through compartments: no view rebuild, no
  // buffer churn. Scale flows through the --editor-scale CSS variable on the
  // host (styles.css consumes it), so source AND preview follow one value.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: [
        lineNumComp.current.reconfigure(settings.lineNumbers ? lineNumbers() : []),
        wrapComp.current.reconfigure(settings.wrap ? EditorView.lineWrapping : []),
        completionsComp.current.reconfigure(
          completions
            ? [
                autocompletion({ override: [fileCompletions(completions)] }),
                referenceHover(completions, onOpenReference, previewLabels),
                completionPreview(onOpenReference, previewLabels),
              ]
            : [],
        ),
      ],
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.lineNumbers, settings.wrap, completions, onOpenReference, t]);

  /**
   * 发布 (the ONLY path that moves HEAD): a conditional PUT creating a new
   * revision. ⌘S deliberately does NOT publish — it stashes the draft.
   */
  const publish = useCallback(async () => {
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
      // The published text is canonical: any stored draft for this path is
      // stale by definition — dropping it keeps the doc-key resync from
      // resurrecting content the user already deleted.
      clearDraft(current.path);
      invalidatePreview(current.path);
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

  // ⌘S = 暂存 (stash): flush the debounce-pending draft NOW and say so.
  // Publishing stays explicit — the 发布 button or ⌘⏎.
  const stashDraft = useCallback(() => {
    const current = stateRef.current;
    if (current.kind !== "dirty" && current.kind !== "error" && current.kind !== "saving") return;
    flushDrafts();
    toast.info(t("draftStoredTitle"), { description: t("draftStoredHint") });
  }, [t]);

  // ⌘S stashes the draft; ⌘⏎ publishes. ⌘S must never create a revision.
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const meta = event.metaKey || event.ctrlKey;
      // A modal owns ⌘S/⌘⏎: publishing or toasting from behind a dialog
      // double-fires whatever the dialog is confirming.
      if (
        meta &&
        (event.target as HTMLElement | null)?.closest?.('[role="dialog"], [role="alertdialog"]')
      ) {
        return;
      }
      if (meta && event.key.toLowerCase() === "s") {
        event.preventDefault();
        stashDraft();
      } else if (meta && event.key === "Enter") {
        event.preventDefault();
        void publish();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [publish, stashDraft]);

  const [saveAsOpen, setSaveAsOpen] = useState(false);
  const openSaveAs = () => {
    const current = stateRef.current;
    if (current.kind !== "clean" && current.kind !== "dirty") return;
    setSaveAsOpen(true);
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
            <ToggleGroupSection
              value={viewMode}
              onChange={(next) => setViewMode(next as "source" | "preview")}
              className="mr-1 flex items-center gap-0.5 rounded-[7px] border p-0.5"
              options={[
                { value: "source", label: t("modeSource"), icon: <SquarePen className="size-3.5 stroke-[1.5]" /> },
                { value: "preview", label: t("modePreview"), icon: <Eye className="size-3.5 stroke-[1.5]" /> },
              ]}
            />
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
              variant="default"
              data-testid="publish-button"
              className="ml-1"
              disabled={state.kind !== "dirty" && state.kind !== "error"}
              onClick={() => void publish()}
            >
              <Send />
              {t("publish")}
              <MenuShortcut>⌘⏎</MenuShortcut>
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
            {onOpenProperties && (
              <Tooltip content={`${t("properties")}（⌘I）`}>
                <Button size="icon" aria-label={t("properties")} onClick={onOpenProperties}>
                  <Info />
                </Button>
              </Tooltip>
            )}
          </div>
        )}
      </div>

      {!inConflict && state.kind !== "empty" && viewMode === "source" && (
        <MarkdownToolbar
          view={viewInstance}
          open={settings.toolbar}
          onToggle={() => updateSettings({ toolbar: !settings.toolbar })}
        />
      )}

      <div
        className="relative min-h-0 flex-1"
        data-editor-font={settings.font}
        style={{ "--editor-scale": settings.scale / 100 } as React.CSSProperties}
      >
        {inConflict && (
          <ConflictView
            state={state}
            stateRef={stateRef}
            onChange={onChange}
            onAuthLost={onAuthLost}
          />
        )}
        <div
          className={`absolute inset-0${inConflict || viewMode === "preview" ? " invisible" : ""}`}
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
              <p className="text-body text-foreground/80">{t("emptyTitle")}</p>
              <p className="mt-1 text-xs">{t("emptyState")}</p>
            </div>
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-3 border-t px-3 py-1.5 text-meta text-faint">
        {state.kind !== "empty" && state.kind !== "loading" && (
          <span className="font-mono">
            {t("stats", {
              chars: stats.chars,
              words: stats.words,
              lines: stats.lines,
            })}
          </span>
        )}
        <span className="flex-1" />
        {inConflict || state.kind === "empty" ? null : (
          <div className="flex items-center gap-2">
            <SliderRow
              ariaLabel={t("scaleLabel")}
              value={settings.scale}
              min={80}
              max={160}
              step={5}
              suffix="%"
              resetLabel={t("scaleReset")}
              onChange={(scale) => updateSettings({ scale })}
              onReset={() => updateSettings({ scale: 100 })}
              className="w-40"
            />
            <Tooltip content={settings.wrap ? t("wrapLabel") : t("overflowLabel")}>
              <Toggle
                pressed={settings.wrap}
                onPressedChange={(pressed) => updateSettings({ wrap: pressed === true })}
                aria-label={settings.wrap ? t("wrapLabel") : t("overflowLabel")}
                className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[pressed]:bg-selected data-[pressed]:text-foreground"
              >
                <WrapText className="size-3.5 stroke-[1.5]" />
              </Toggle>
            </Tooltip>
          </div>
        )}
        <span className="font-mono">
          {inConflict || state.kind === "empty" || state.kind === "loading"
            ? null
            : t("revLabel", { revision: state.baseRevision.slice(0, 8) })}
        </span>
      </div>

      <SaveAsDialog
        open={saveAsOpen}
        onOpenChange={setSaveAsOpen}
        defaultPath={state.kind === "empty" || state.kind === "loading" ? "" : state.path}
        currentText={buffer}
        onSaved={(result) => {
          clearDraft(result.path);
          onStateChange(freshEditorState(result.path, buffer, result.revision));
        }}
        onAuthLost={onAuthLost}
      />
    </div>
  );
}
