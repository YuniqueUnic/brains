import { EditorState as CodeMirrorState } from "@codemirror/state";
import { MergeView, unifiedMergeView } from "@codemirror/merge";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorView } from "@codemirror/view";
import { GitMerge } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ApiError } from "../../api/client";
import { Button } from "../ui/button";
import {
  baseExtensions,
  findPanelLabels,
  freshEditorState,
  type EditorState,
} from "./editor-core";
import { cn } from "../../lib/utils";

/*
 * Conflict resolution with VSCode-style diff views (DESIGN.md §26-34):
 * "split" shows remote and local side-by-side with synced chunk highlights;
 * "inline" shows the local buffer against the remote original with
 * per-chunk accept/reject. The merge editor owns the buffer while the
 * conflict is open — text typed here flows back into `localText` through
 * the same state machine.
 */

export type ConflictMode = "split" | "inline";

export function ConflictView({
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
            EditorView.lineWrapping,
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
        extensions: [
          ...baseExtensions(onDocChange, () => undefined, () => findPanelLabels(t)),
          EditorView.lineWrapping,
        ],
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
