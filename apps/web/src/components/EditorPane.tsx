import { useCallback, useEffect, useRef } from "react";
import { EditorState as CodeMirrorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { api, ApiError } from "../api/client";

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

export default function EditorPane({
  state,
  onChange,
  onStateChange,
  onAuthLost,
}: {
  state: EditorState;
  onChange: (next: EditorState) => void;
  onStateChange: (next: EditorState) => void;
  /** Session expired mid-edit: the workspace logs out and shows a banner. */
  onAuthLost?: () => void;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const stateRef = useRef<EditorState>(state);
  stateRef.current = state;

  useEffect(() => {
    if (!hostRef.current) return;
    const view = new EditorView({
      parent: hostRef.current,
      state: CodeMirrorState.create({
        doc: currentDoc(stateRef.current),
        extensions: [
          markdown({ base: markdownLanguage }),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            const text = update.state.doc.toString();
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
          }),
        ],
      }),
    });
    viewRef.current = view;
    return () => view.destroy();
    // Editor instance lives across file switches; doc is swapped below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
          remoteText = "(无法读取远端当前内容)";
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
  }, [onStateChange]);

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
    const path = window.prompt("新文件路径（vault 内相对路径）:", current.path);
    if (!path) return;
    try {
      const result = await api.saveText(path, current.text, { mode: "create" });
      onStateChange(freshEditorState(result.path, current.text, result.revision));
    } catch (error) {
      window.alert(error instanceof Error ? error.message : String(error));
    }
  };

  // The CodeMirror host must stay mounted through EVERY state — including
  // conflict, which renders its panel as a sibling and hides (never unmounts)
  // the editor. Unmounting would orphan the one EditorView created on mount
  // and leave a dead editor after conflict resolution.
  const statusLabel = statusText(state);
  const inConflict = state.kind === "conflict";

  return (
    <div className="editor-pane" data-testid="editor-pane">
      <div className="editor-toolbar">
        <span className="editor-path" data-testid="editor-path">
          {state.kind === "empty" ? "" : state.path}
        </span>
        <span
          className={`editor-status status-${state.kind}`}
          data-testid="editor-status"
        >
          {statusLabel}
        </span>
        <div className="editor-actions">
          {inConflict || state.kind === "empty" ? null : (
            <>
              <button
                data-testid="save-button"
                disabled={state.kind !== "dirty" && state.kind !== "error"}
                onClick={() => void save()}
              >
                保存
              </button>
              {state.kind === "clean" || state.kind === "dirty" ? (
                <button onClick={() => void createSave()}>另存为…</button>
              ) : null}
            </>
          )}
        </div>
      </div>
      <div className="editor-body">
        {inConflict && <ConflictView state={state} stateRef={stateRef} onChange={onChange} />}
        <div
          className={`codemirror-host${inConflict ? " host-hidden" : ""}`}
          data-testid="codemirror-host"
          ref={hostRef}
        />
        {state.kind === "empty" && (
          <div className="editor-empty muted">从左侧选择或新建一个文件</div>
        )}
      </div>
    </div>
  );
}

function ConflictView({
  state,
  stateRef,
  onChange,
}: {
  state: Extract<EditorState, { kind: "conflict" }>;
  stateRef: React.RefObject<EditorState>;
  onChange: (next: EditorState) => void;
}) {
  // True while THIS conflict still owns the editor (the human may have
  // opened another file while the request was in flight).
  const stillMine = () =>
    stateRef.current.kind === "conflict" &&
    stateRef.current.path === state.path &&
    stateRef.current.remoteRevision === state.remoteRevision;

  const saveWithRemoteBase = async () => {
    try {
      const result = await api.saveText(state.path, state.localText, {
        mode: "update",
        baseRevision: state.remoteRevision,
      });
      if (!stillMine()) return;
      onChange(freshEditorState(state.path, state.localText, result.revision));
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
          notice = "远端已再次推进，但暂无法读取其内容；请重试后再决定。";
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
    <div className="conflict-view" data-testid="conflict-view">
      <p className="error">
        保存期间文件已被其他客户端修改（当前远端版本 {state.remoteRevision.slice(0, 8)}…）。
        请比较下方三个版本后选择操作；不会自动覆盖。
      </p>
      {state.notice && <p className="error">{state.notice}</p>}
      <div className="conflict-columns">
        <div>
          <h3>基线（你的起点）</h3>
          <textarea readOnly value={state.baseText} />
        </div>
        <div>
          <h3>本地（你的修改）</h3>
          <textarea
            value={state.localText}
            onChange={(e) => onChange({ ...state, localText: e.target.value })}
          />
        </div>
        <div>
          <h3>远端（当前版本）</h3>
          <textarea readOnly value={state.remoteText} />
        </div>
      </div>
      <div className="conflict-actions">
        <button data-testid="conflict-keep-local" onClick={() => void saveWithRemoteBase()}>
          以本地内容保存（基于远端当前版本）
        </button>
        <button
          data-testid="conflict-take-remote"
          onClick={takeRemote}
          disabled={!remoteUsable}
          title={remoteUsable ? undefined : "远端内容暂不可读取，不能采用"}
        >
          放弃本地，采用远端
        </button>
      </div>
    </div>
  );
}

function statusText(state: EditorState): string {
  switch (state.kind) {
    case "empty":
      return "未打开文件";
    case "loading":
      return "加载中…";
    case "clean":
      return "已保存";
    case "dirty":
      return "未保存修改";
    case "saving":
      return "保存中…";
    case "conflict":
      return "版本冲突";
    case "error":
      return state.message ?? "保存失败";
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
