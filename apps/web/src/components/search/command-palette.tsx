import { Command } from "cmdk";
import {
  FilePlus2,
  FileText,
  LoaderCircle,
  Search,
  Upload,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Entry, type SearchHit } from "../../api/client";
import { cn } from "../../lib/utils";
import { Dialog, DialogContent } from "../ui/dialog";
import type { UploadIntent } from "../shell/top-bar";

/*
 * ⌘K Command Palette (DESIGN.md §20-25): one HUD for jumping to documents,
 * running actions, and full-text search (literal/FTS). Local matches render
 * instantly; Enter (or a short debounce) runs the server search and shows
 * highlighted snippets.
 */

type FullTextState =
  | { kind: "idle" }
  | { kind: "error" }
  | { kind: "running" }
  | {
      kind: "done";
      query: string;
      items: SearchHit[];
      incomplete: boolean;
      building: boolean;
    };

export function CommandPalette({
  open,
  onOpenChange,
  files,
  searchMode,
  onSearchModeChange,
  onOpenFile,
  onNewDocument,
  onUpload,
  tag,
  onClearTag,
}: {
  tag?: string | null;
  onClearTag?: () => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  files: Entry[];
  searchMode: "literal" | "fts";
  onSearchModeChange: (mode: "literal" | "fts") => void;
  onOpenFile: (path: string) => void;
  onNewDocument: () => void;
  onUpload: (intent: UploadIntent) => void;
}) {
  const { t } = useTranslation("workspace");
  const [query, setQuery] = useState("");
  const [fullText, setFullText] = useState<FullTextState>({ kind: "idle" });
  const runIdRef = useRef(0);

  const localMatches = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return files.slice(0, 8);
    return files
      .filter(
        (file) =>
          file.path.toLowerCase().includes(q) || (file.title ?? "").toLowerCase().includes(q),
      )
      .slice(0, 8);
  }, [files, query]);

  const runFullText = async (raw: string) => {
    const q = raw.trim();
    const runId = runIdRef.current + 1;
    runIdRef.current = runId;
    if (!q && !tag) {
      setFullText({ kind: "idle" });
      return;
    }
    setFullText({ kind: "running" });
    try {
      if (tag) {
        let cursor: string | null = null;
        const items: SearchHit[] = [];
        let pages = 0;
        do {
          const page = await api.listFiles({ source: "index", tag, cursor });
          items.push(...page.items
            .filter(file => `${file.path} ${file.title ?? ""}`.toLowerCase().includes(q.toLowerCase()))
            .map(file => ({
              path: file.path,
              indexed_revision: file.revision,
              snippet: file.title ?? "",
            })));
          cursor = page.next_cursor;
          pages += 1;
        } while (cursor && pages < 20 && runIdRef.current === runId);
        if (runIdRef.current === runId) {
          setFullText({ kind: "done", query: `#${tag}`, items, incomplete: !!cursor, building: false });
        }
        return;
      }
      const page = await api.search({ q, mode: searchMode });
      let items = page.items;
      let cursor = page.next_cursor;
      let guard = 0;
      while (cursor && guard < 20) {
        const next = await api.search({ q, mode: searchMode, cursor });
        items = items.concat(next.items);
        cursor = next.next_cursor;
        guard += 1;
      }
      if (runIdRef.current !== runId) return;
      setFullText({
        kind: "done",
        query: q,
        items,
        incomplete: Boolean(cursor) || !page.complete,
        building: page.index_state === "building",
      });
    } catch {
      // A failed search must not pose as "no matches".
      if (runIdRef.current === runId) setFullText({ kind: "error" });
    }
  };

  // Reset per open; debounced auto-search keeps Enter instant but optional.
  useEffect(() => {
    if (!open) {
      setQuery("");
      setFullText({ kind: "idle" });
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const q = query.trim();
    if (q.length < 2 && !tag) {
      setFullText({ kind: "idle" });
      return;
    }
    const timer = window.setTimeout(() => void runFullText(q), 300);
    return () => {
      window.clearTimeout(timer);
      runIdRef.current += 1;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, searchMode, open, tag]);

  const showFullText = fullText.kind === "running" || fullText.kind === "done";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        aria-label={t("paletteOpen")}
        className="top-[18%] w-[min(640px,94vw)] translate-y-0 overflow-hidden p-0"
      >
        <Command shouldFilter={false} label={t("paletteOpen")} className="flex flex-col">
          <div className="flex items-center gap-2 border-b px-4">
            <Search className="size-4 shrink-0 stroke-[1.5] text-faint" />
            {tag && <button type="button" className="document-tag" onClick={onClearTag} aria-label={t("clearTag", { tag })}>#{tag} ×</button>}
            <Command.Input
              data-testid="search-box"
              value={query}
              onValueChange={setQuery}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void runFullText(query);
                }
              }}
              placeholder={t("palettePlaceholder")}
              className="h-12 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground/70"
              autoFocus
            />
            {fullText.kind === "running" && (
              <LoaderCircle className="size-4 shrink-0 animate-spin stroke-[1.5] text-faint" />
            )}
            <kbd>Esc</kbd>
          </div>

          <Command.List className="max-h-[380px] overflow-y-auto p-1.5">
            {fullText.kind !== "error" && fullText.kind !== "running" && (
              <Command.Empty className="px-3 py-6 text-center text-xs text-muted-foreground">
                {t("paletteNoMatches")}
              </Command.Empty>
            )}
            {fullText.kind === "error" && (
              <div className="px-3 py-6 text-center text-xs text-muted-foreground" role="status">
                {t("paletteSearchFailed")}
              </div>
            )}

            {showFullText && (
              <PaletteGroup
                label={
                  fullText.kind === "done"
                    ? t("searchResultsTitle", {
                        query: fullText.query,
                        count: fullText.items.length,
                      }) +
                      (fullText.incomplete ? t("searchIncomplete") : "") +
                      (fullText.building ? t("searchBuilding") : "")
                    : t("paletteSearching")
                }
                testid="search-results"
              >
                {fullText.kind === "done" &&
                  fullText.items.map((hit) => (
                    <Command.Item
                      key={hit.path + hit.indexed_revision}
                      value={`hit ${hit.path}`}
                      onSelect={() => {
                        onOpenFile(hit.path);
                        onOpenChange(false);
                      }}
                      className="cursor-pointer"
                      asChild
                    >
                      <button
                        type="button"
                        data-testid="search-hit"
                        className="w-full rounded-md px-2.5 py-2 text-left outline-none aria-selected:bg-muted"
                        title={hit.path}
                      >
                        <span className="block truncate font-mono text-xs text-foreground">
                          {hit.path}
                        </span>
                        <span className="mt-0.5 block truncate text-meta-lg text-muted-foreground">
                          <Snippet text={hit.snippet} query={fullText.query} />
                        </span>
                      </button>
                    </Command.Item>
                  ))}
              </PaletteGroup>
            )}

            {!tag && !showFullText && localMatches.length > 0 && (
              <PaletteGroup label={t("paletteDocuments")}>
                {localMatches.map((file) => (
                  <Command.Item
                    key={file.path}
                    value={`doc ${file.path} ${file.title ?? ""}`}
                    onSelect={() => {
                      onOpenFile(file.path);
                      onOpenChange(false);
                    }}
                    className="cursor-pointer"
                    asChild
                  >
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left outline-none aria-selected:bg-muted"
                    >
                      <FileText className="size-4 shrink-0 stroke-[1.5] text-faint" />
                      <span className="min-w-0 flex-1 truncate">
                        {file.title ? (
                          <>
                            <span className="text-body">{file.title}</span>
                            <span className="ml-2 font-mono text-meta text-faint">
                              {file.path}
                            </span>
                          </>
                        ) : (
                          <span className="font-mono text-xs">{file.path}</span>
                        )}
                      </span>
                    </button>
                  </Command.Item>
                ))}
              </PaletteGroup>
            )}

            {!tag && query.trim() === "" && (
              <PaletteGroup label={t("paletteActions")}>
                <Command.Item
                  value="action new document"
                  onSelect={() => {
                    onNewDocument();
                    onOpenChange(false);
                  }}
                  asChild
                >
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-body outline-none aria-selected:bg-muted"
                  >
                    <FilePlus2 className="size-4 stroke-[1.5] text-faint" />
                    {t("newDocument")}
                  </button>
                </Command.Item>
                <Command.Item
                  value="action upload files"
                  onSelect={() => {
                    onUpload("files");
                    onOpenChange(false);
                  }}
                  asChild
                >
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-body outline-none aria-selected:bg-muted"
                  >
                    <Upload className="size-4 stroke-[1.5] text-faint" />
                    {t("uploadFiles")}
                  </button>
                </Command.Item>
                <Command.Item
                  value="action upload folder"
                  onSelect={() => {
                    onUpload("folder");
                    onOpenChange(false);
                  }}
                  asChild
                >
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-body outline-none aria-selected:bg-muted"
                  >
                    <Upload className="size-4 stroke-[1.5] text-faint" />
                    {t("uploadFolder")}
                  </button>
                </Command.Item>
              </PaletteGroup>
            )}
          </Command.List>

          <div className="flex items-center gap-2 border-t px-3 py-2 text-meta text-faint">
            {!tag && <><ModeChip
              active={searchMode === "literal"}
              testid="mode-literal"
              onClick={() => onSearchModeChange("literal")}
            >
              {t("modeLiteral")}
            </ModeChip>
            <ModeChip
              active={searchMode === "fts"}
              testid="mode-fts"
              onClick={() => onSearchModeChange("fts")}
            >
              {t("modeFts")}
            </ModeChip></>}
            <span className="ml-auto flex items-center gap-2">
              <span>↑↓ {t("paletteNavigate")}</span>
              <span>↵ {t("paletteOpen")}</span>
            </span>
          </div>
        </Command>
      </DialogContent>
    </Dialog>
  );
}

function PaletteGroup({
  label,
  testid,
  children,
}: {
  label: string;
  testid?: string;
  children: React.ReactNode;
}) {
  return (
    // The contents wrapper keeps cmdk's layout while giving the e2e suite a
    // stable container that includes the heading and the hit buttons.
    <div data-testid={testid} className={testid ? "contents" : undefined}>
      <Command.Group
        heading={
          <div className="truncate px-2.5 pt-2 pb-1 text-meta font-medium text-faint">
            {label}
          </div>
        }
        className="py-0.5"
      >
        {children}
      </Command.Group>
    </div>
  );
}

function ModeChip({
  active,
  testid,
  onClick,
  children,
}: {
  active: boolean;
  testid: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      data-testid={testid}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "cursor-pointer rounded-md px-2 py-0.5 text-meta font-medium transition-colors",
        active
          ? "bg-primary/12 text-primary ring-1 ring-primary/30"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** Bold the query inside the snippet (DESIGN.md §23: bold, never highlighter). */
function Snippet({ text, query }: { text: string; query: string }) {
  const q = query.trim();
  if (!q) return <>{text.slice(0, 160)}</>;
  const index = text.toLowerCase().indexOf(q.toLowerCase());
  if (index < 0) return <>{text.slice(0, 160)}</>;
  const start = Math.max(0, index - 40);
  const prefix = start > 0 ? "…" : "";
  return (
    <>
      {prefix}
      {text.slice(start, index)}
      <mark className="bg-transparent font-semibold text-foreground">{q}</mark>
      {text.slice(index + q.length, index + q.length + 120)}
    </>
  );
}
