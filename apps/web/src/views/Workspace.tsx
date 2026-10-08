import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  ApiError,
  isTextMedia,
  type Entry,
  type SearchHit,
  type SessionInfo,
} from "../api/client";
import EditorPane, { type EditorState } from "../components/EditorPane";
import InfoPanel from "../components/InfoPanel";

/**
 * Main workspace: sidebar (search + file tree + trash), editor, info panel.
 * Layout mirrors design §13.1 without dragging in a router.
 */
export default function Workspace({
  session,
  onLogout,
}: {
  session: SessionInfo;
  onLogout: () => void;
}) {
  const [files, setFiles] = useState<Entry[]>([]);
  const [filesTruncated, setFilesTruncated] = useState(false);
  const [showTrash, setShowTrash] = useState(false);
  const [editor, setEditor] = useState<EditorState>({ kind: "empty" });
  const [searchQuery, setSearchQuery] = useState("");
  const [searchMode, setSearchMode] = useState<"literal" | "fts">("literal");
  const [searchResults, setSearchResults] = useState<SearchHit[] | null>(null);
  const [searchIncomplete, setSearchIncomplete] = useState(false);
  const [searchBuilding, setSearchBuilding] = useState(false);
  const [banner, setBanner] = useState<string | null>(null);
  const [showNewFile, setShowNewFile] = useState(false);
  const [newFilePath, setNewFilePath] = useState("");
  const [showTrashConfirm, setShowTrashConfirm] = useState(false);
  // Opening another file with unsaved/conflicting text in the editor requires
  // an explicit discard — the confirm bar reuses the trash pattern.
  const [pendingOpen, setPendingOpen] = useState<string | null>(null);
  const editorRef = useRef(editor);
  editorRef.current = editor;

  const fail = useCallback(
    (error: unknown) => {
      // An expired session has no in-page recovery: back to login.
      if (error instanceof ApiError && error.status === 401) {
        onLogout();
        return;
      }
      setBanner(error instanceof Error ? error.message : String(error));
    },
    [onLogout],
  );

  const refreshFiles = useCallback(async () => {
    // Walk pages up to a sane bound; anything left is surfaced as truncated
    // instead of silently hiding files (design §11.1: no fake completion).
    let items: Entry[] = [];
    let cursor: string | null = null;
    let guard = 0;
    do {
      const page = await api.listFiles({
        source: "store",
        include_deleted: showTrash,
        limit: 100,
        cursor,
      });
      items = items.concat(page.items);
      cursor = page.next_cursor;
      guard += 1;
    } while (cursor && guard < 20);
    setFiles(items);
    setFilesTruncated(Boolean(cursor));
  }, [showTrash]);

  useEffect(() => {
    void refreshFiles();
  }, [refreshFiles]);

  const requestOpen = (path: string) => {
    const current = editorRef.current;
    // `saving` counts too: keystrokes typed during the flight have not been
    // sent yet and would be dropped without a word.
    if (
      current.kind === "dirty" ||
      current.kind === "conflict" ||
      current.kind === "error" ||
      current.kind === "saving"
    ) {
      setPendingOpen(path);
      return;
    }
    void openFile(path);
  };

  const openFile = async (path: string) => {
    setEditor({ kind: "loading", path });
    try {
      // Text-only editor: refuse binary media instead of decoding bytes into
      // mojibake that a later save would corrupt (design §13.3).
      const meta = await api.readMeta(path);
      if (!isTextMedia(meta.media_type)) {
        setBanner(
          `${path} 是二进制文件（${meta.media_type}），网页端仅提供下载；请用 CLI put/get 处理。`,
        );
        setEditor({ kind: "empty" });
        return;
      }
      const { text, etag } = await api.readText(path);
      // The two calls are separate: if the file moved between them (e.g. a
      // rival replaced it with binary bytes), reopen once against the truth.
      const latest = await api.readMeta(path);
      if (latest.revision !== etag) {
        if (!isTextMedia(latest.media_type)) {
          setBanner(`${path} 已被替换为二进制内容，网页端仅提供下载。`);
          setEditor({ kind: "empty" });
          return;
        }
        const fresh = await api.readText(path);
        setEditor({ kind: "clean", path, text: fresh.text, baseText: fresh.text, baseRevision: fresh.etag });
        return;
      }
      setEditor({ kind: "clean", path, text, baseText: text, baseRevision: etag });
    } catch (error) {
      if (error instanceof ApiError && error.status === 410) {
        setBanner(`${path} 在回收站中；勾选“回收站”可查看与恢复。`);
        setEditor({ kind: "empty" });
        return;
      }
      fail(error);
      setEditor({ kind: "empty" });
    }
  };

  const newFile = async () => {
    const path = newFilePath.trim();
    if (!path) return;
    const current = editorRef.current;
    if (current.kind === "dirty" || current.kind === "conflict" || current.kind === "error") {
      setBanner("有未保存修改：请先保存（或放弃）后再新建文件。");
      return;
    }
    try {
      const result = await api.saveText(path, "", { mode: "create" });
      setNewFilePath("");
      setShowNewFile(false);
      await refreshFiles();
      setEditor({ kind: "clean", path: result.path, text: "", baseText: "", baseRevision: result.revision });
    } catch (error) {
      fail(error);
    }
  };

  const trashCurrent = async () => {
    if (editor.kind !== "clean" && editor.kind !== "dirty") return;
    setShowTrashConfirm(false);
    try {
      await api.trash(editor.path, editor.baseRevision);
      setEditor({ kind: "empty" });
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

  const uploadAttachment = async (file: File) => {
    const path = `attachments/${file.name}`;
    try {
      const buffer = await file.arrayBuffer();
      await api.saveBinary(
        path,
        buffer,
        { mode: "create" },
        file.type || "application/octet-stream",
      );
      setBanner(`${path} 已上传。`);
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

  const runSearch = async () => {
    if (!searchQuery.trim()) return;
    try {
      const page = await api.search({ q: searchQuery, mode: searchMode });
      let items = page.items;
      let cursor = page.next_cursor;
      // The UI walks literal windows for the user up to a sane bound; any
      // remaining cursor is surfaced as "未翻完" instead of faking completion.
      let guard = 0;
      while (cursor && guard < 20) {
        const next = await api.search({ q: searchQuery, mode: searchMode, cursor });
        items = items.concat(next.items);
        cursor = next.next_cursor;
        guard += 1;
      }
      setSearchResults(items);
      setSearchIncomplete(Boolean(cursor) || !page.complete);
      setSearchBuilding(page.index_state === "building");
    } catch (error) {
      fail(error);
    }
  };

  const restoreFromTrash = async (path: string) => {
    try {
      // The tombstone itself has no content: restore the newest contentful
      // revision reachable from it (design §8.1: restore takes a live source).
      const history = await api.history(path);
      const anchor = history.anchor_revision;
      let source: string | null = null;
      let cursor: string | null = null;
      let pages = 0;
      do {
        const page = await api.history(path, cursor ?? undefined);
        const newestContentful = page.items.find((rev) => rev.action !== "trash");
        if (newestContentful) {
          source = newestContentful.revision;
          break;
        }
        cursor = page.next_cursor;
        pages += 1;
      } while (cursor && pages < 10);
      if (!source) {
        setBanner(`${path} 没有可恢复的历史版本。`);
        return;
      }
      await api.restore(path, source, anchor);
      setBanner(`${path} 已恢复为新版本。`);
      await refreshFiles();
    } catch (error) {
      fail(error);
    }
  };

  return (
    <div className="workspace">
      <header className="topbar">
        <span className="brand">BrainStore</span>
        <input
          className="search-box"
          data-testid="search-box"
          placeholder="全库搜索（Enter 执行）"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void runSearch();
          }}
        />
        <div className="mode-toggle" role="group">
          <button
            data-testid="mode-literal"
            className={searchMode === "literal" ? "active" : ""}
            onClick={() => setSearchMode("literal")}
          >
            字面
          </button>
          <button
            data-testid="mode-fts"
            className={searchMode === "fts" ? "active" : ""}
            onClick={() => setSearchMode("fts")}
          >
            FTS
          </button>
        </div>
        <div className="topbar-actions">
          <button
            data-testid="new-file"
            onClick={() => setShowNewFile((value) => !value)}
          >
            新建
          </button>
          <label className="upload-button">
            上传
            <input
              type="file"
              data-testid="upload-input"
              hidden
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (file) void uploadAttachment(file);
                e.target.value = "";
              }}
            />
          </label>
          <button
            data-testid="toggle-trash"
            onClick={() => setShowTrash((value) => !value)}
          >
            {showTrash ? "退出回收站" : "回收站"}
          </button>
          <span className="muted">{session.actor}</span>
          <button data-testid="logout" onClick={onLogout}>
            退出
          </button>
        </div>
      </header>

      {banner && (
        <div className="banner" data-testid="banner">
          {banner}
          <button onClick={() => setBanner(null)}>×</button>
        </div>
      )}

      {showNewFile && (
        <div className="inline-dialog" data-testid="new-file-dialog">
          <input
            data-testid="new-file-path"
            placeholder="新文件路径，例如 inbox/idea.md"
            value={newFilePath}
            onChange={(e) => setNewFilePath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void newFile();
            }}
            autoFocus
          />
          <button data-testid="new-file-confirm" onClick={() => void newFile()}>
            创建
          </button>
          <button onClick={() => setShowNewFile(false)}>取消</button>
        </div>
      )}

      {showTrashConfirm && (
        <div className="banner" data-testid="trash-confirm">
          <span>移入回收站？未保存的修改会一并丢弃（无法恢复）；历史版本保留，可随时恢复。</span>
          <button data-testid="trash-confirm-yes" onClick={() => void trashCurrent()}>
            确认
          </button>
          <button onClick={() => setShowTrashConfirm(false)}>取消</button>
        </div>
      )}

      {pendingOpen && (
        <div className="banner" data-testid="open-confirm">
          <span>当前有未保存修改或未解决的冲突，放弃并打开 {pendingOpen}？</span>
          <button
            data-testid="open-confirm-yes"
            onClick={() => {
              const path = pendingOpen;
              setPendingOpen(null);
              void openFile(path);
            }}
          >
            放弃并打开
          </button>
          <button onClick={() => setPendingOpen(null)}>取消</button>
        </div>
      )}

      <div className="columns">
        <nav className="sidebar" data-testid="file-list">
          {files.length === 0 && <p className="muted">没有文件</p>}
          {filesTruncated && (
            <p className="muted" data-testid="files-truncated">
              （列表未翻完，可用搜索定位其余文件）
            </p>
          )}
          <ul>
            {files.map((file) => (
              <li key={file.path}>
                <button
                  className="file-item"
                  onClick={() => requestOpen(file.path)}
                >
                  <span className="file-path">{file.path}</span>
                  {file.state === "trashed" && (
                    <button
                      className="restore-button"
                      onClick={(e) => {
                        e.stopPropagation();
                        void restoreFromTrash(file.path);
                      }}
                    >
                      恢复
                    </button>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        {searchResults && (
          <div className="search-results" data-testid="search-results">
            <h3>
              搜索“{searchQuery}” — {searchResults.length} 个命中
              {searchIncomplete && <span className="error">（未翻完全库）</span>}
              {searchBuilding && <span className="muted">（索引建库中，结果可能不全）</span>}
            </h3>
            <ul>
              {searchResults.map((hit) => (
                <li key={hit.path + hit.indexed_revision}>
                  <button onClick={() => requestOpen(hit.path)}>{hit.path}</button>
                  <p className="muted">{hit.snippet.slice(0, 160)}</p>
                </li>
              ))}
            </ul>
          </div>
        )}

        <main className="main">
          <EditorPane
            state={editor}
            onChange={setEditor}
            onStateChange={setEditor}
            onAuthLost={() => {
              setBanner("登录已过期，请重新登录。");
              onLogout();
            }}
          />
          {editor.kind === "clean" || editor.kind === "dirty" ? (
            <button
              className="danger"
              data-testid="trash-current"
              onClick={() => setShowTrashConfirm(true)}
            >
              移入回收站
            </button>
          ) : null}
        </main>

        <InfoPanel
          path={infoPath(editor)}
          revision={infoRevision(editor)}
          onRestore={(revision) => {
            // Restoring replaces the buffer: only a clean editor may lose it
            // silently; dirty/conflict editors must resolve first.
            if (editor.kind !== "clean") {
              setBanner("有未保存修改：请先保存（或放弃修改）后再恢复历史版本。");
              return;
            }
            void (async () => {
              try {
                const result = await api.restore(editor.path, revision, editor.baseRevision);
                const { text, etag } = await api.readText(editor.path);
                void result;
                setEditor({
                  kind: "clean",
                  path: editor.path,
                  text,
                  baseText: text,
                  baseRevision: etag,
                });
                setBanner(`已恢复。`);
                await refreshFiles();
              } catch (error) {
                fail(error);
              }
            })();
          }}
        />
      </div>
    </div>
  );
}

function infoPath(state: EditorState): string | null {
  return state.kind === "empty" || state.kind === "loading" ? null : state.path;
}

function infoRevision(state: EditorState): string | null {
  switch (state.kind) {
    case "clean":
    case "dirty":
    case "error":
    case "saving":
      return state.baseRevision;
    default:
      return null;
  }
}
