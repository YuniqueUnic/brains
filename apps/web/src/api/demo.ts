import { parseMarkdownDocument } from "../lib/document-metadata";
import { createMarkdownParser } from "../lib/markdown-parser";
import { resolveReference } from "../components/markdown/references";
import {
  ApiError,
  isTextMedia,
  type Entry,
  type FilePage,
  type HistoryPage,
  type LinkItem,
  type MutationResult,
  type Precondition,
  type SearchPage,
  type SessionInfo,
  type TransferResult,
} from "./client";

/** Page-local demo storage. Raw bytes are authoritative; refresh resets the files. */
interface DemoContent {
  bytes: Uint8Array<ArrayBuffer>;
  sha256: string;
  media_type: string;
}

interface DemoRevision {
  revision: string;
  generation: number;
  parent: string | null;
  action: "put" | "trash" | "restore" | "copy";
  created_at: string;
  content?: DemoContent;
}

interface DemoFile {
  revisions: DemoRevision[];
}

const SEEDS: { path: string; mediaType: string; text: string }[] = [
  {
    path: "notes/rust-ci-cache.md",
    mediaType: "text/markdown",
    text: `---\ntitle: "Rust CI 缓存策略"\ntags: [ci, cache]\n---\n\n# Rust CI 缓存策略\n\n用 **sccache** 复用增量编译产物，链接器换 mold。\n\n- 共享缓存卷指向固定目录\n- 命中率看 job summary\n\n详见 [[benchmark-notes]] 的分片对比。\n`,
  },
  {
    path: "notes/benchmark-notes.md",
    mediaType: "text/markdown",
    text: `---\ntitle: "Benchmark 记录"\ntags: [benchmark]\n---\n\n# Benchmark 记录\n\n对比 [[rust-ci-cache]] 两种链接器：\n\n| 链接器 | 冷构建 | 热构建 |\n|---|---|---|\n| ld | 212s | 41s |\n| mold | 198s | 12s |\n`,
  },
  {
    path: "notes/weekly/2026-W41.md",
    mediaType: "text/markdown",
    text: `---\ntitle: "周报 W41"\n---\n\n# 周报 W41\n\n调了 CI 的缓存策略，热构建 41s → 12s。\n`,
  },
  {
    path: "inbox/idea.md",
    mediaType: "text/markdown",
    text: `# 收件箱\n\n- 给 benchmark 脚本加火焰图\n- 试试 [[rust-ci-cache]] 里的方案在本机的表现\n`,
  },
];


const store = new Map<string, DemoFile>();
let revisionCounter = 0;

function nextRevision(): string {
  return (++revisionCounter).toString(16).padStart(8, "0") + "demo";
}

function head(file: DemoFile): DemoRevision {
  return file.revisions[0];
}

function textOf(content?: DemoContent): string {
  return content && isTextMedia(content.media_type) ? new TextDecoder().decode(content.bytes) : "";
}

async function contentOf(body: string | ArrayBuffer, mediaType: string): Promise<DemoContent> {
  const bytes = typeof body === "string" ? new TextEncoder().encode(body) : new Uint8Array(body).slice();
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return {
    bytes,
    sha256: Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join(""),
    media_type: mediaType,
  };
}

function headEntry(path: string, file: DemoFile): Entry {
  const top = head(file);
  return {
    path,
    revision: top.revision,
    generation: top.generation,
    state: top.action === "trash" ? "trashed" : "live",
    updated_at: top.created_at,
    size: top.content?.bytes.byteLength,
    sha256: top.content?.sha256,
    media_type: top.content?.media_type,
    kind: top.content && isTextMedia(top.content.media_type) ? "note" : "binary",
    title: parseMarkdownDocument(textOf(top.content)).metadata?.title,
  };
}

/** Call only after async work and preconditions: publishing a revision is synchronous. */
function commit(path: string, action: DemoRevision["action"], content?: DemoContent): MutationResult {
  const previous = store.get(path);
  const revision: DemoRevision = {
    revision: nextRevision(),
    generation: previous ? head(previous).generation + 1 : 1,
    parent: previous ? head(previous).revision : null,
    action,
    created_at: new Date().toISOString(),
    content,
  };
  const file = { revisions: [revision, ...(previous?.revisions ?? [])] };
  store.set(path, file);
  return { ...headEntry(path, file), index_state: "ready", operation_id: nextRevision() };
}

let ready: Promise<void[]> | undefined;

function initialize(): Promise<void[]> {
  return ready ??= Promise.all(SEEDS.map(async (item) => {
    const content = await contentOf(item.text, item.mediaType);
    commit(item.path, "put", content);
  }));
}

async function latency(): Promise<void> {
  await initialize();
  await new Promise((resolve) => window.setTimeout(resolve, 80));
}

function requireFile(path: string): DemoFile {
  const file = store.get(path);
  if (!file) throw new ApiError(404, "NOT_FOUND", `${path} 不存在`);
  return file;
}

function requireLive(path: string): DemoFile {
  const file = requireFile(path);
  if (head(file).action === "trash") throw new ApiError(410, "GONE", `${path} 已在回收站`);
  return file;
}

function checkRevision(file: DemoFile, expected: string): void {
  if (head(file).revision !== expected) {
    throw new ApiError(412, "REVISION_CONFLICT", "基线已过期", head(file).revision);
  }
}

async function putContent(path: string, body: string | ArrayBuffer, precondition: Precondition, mediaType: string): Promise<MutationResult> {
  await latency();
  const content = await contentOf(body, mediaType);
  // Hashing may yield: inspect the current HEAD only after it completes.
  const existing = store.get(path);
  if (precondition.mode === "create") {
    if (existing) throw new ApiError(412, "ALREADY_EXISTS", `${path} 已存在`, head(existing).revision);
  } else {
    checkRevision(requireFile(path), precondition.baseRevision);
  }
  return commit(path, "put", content);
}

async function transfer(source: string, target: string, expected: string, action: "copy" | "move"): Promise<TransferResult> {
  await latency();
  const file = requireLive(source);
  checkRevision(file, expected);
  const operationId = nextRevision();
  const existing = store.get(target);
  if (existing) {
    throw new ApiError(412, "REVISION_CONFLICT", `${target} 已存在`, head(existing).revision);
  }
  const copied = commit(target, "copy", head(file).content);
  const trashed = action === "move" ? commit(source, "trash") : undefined;
  return {
    copied: true, source_trashed: !!trashed, partial: false, operation_id: operationId,
    target_revision: copied.revision, source_revision: trashed?.revision ?? expected,
  };
}

const session: SessionInfo = { actor: "demo", scopes: ["read:/", "write:/", "admin:/"] };

export const demoApi = {
  fileUrl(path: string): string {
    const url = new URL(window.location.href);
    url.search = "";
    url.hash = "";
    url.searchParams.set("file", path);
    return url.href;
  },
  async createSession(_token: string): Promise<SessionInfo> {
    await latency();
    return session;
  },
  async getSession(): Promise<SessionInfo> {
    await initialize();
    return session;
  },
  async deleteSession(): Promise<void> {
    await latency();
  },
  async listFiles(query: {
    prefix?: string; cursor?: string | null; limit?: number; include_deleted?: boolean;
    source?: "store" | "index"; tags?: string[]; sort?: string;
  }): Promise<FilePage> {
    await latency();
    const items = [...store].filter(([path, file]) => {
      if (head(file).action === "trash" && !query.include_deleted) return false;
      if (!path.startsWith(query.prefix ?? "")) return false;
      const tags = parseMarkdownDocument(textOf(head(file).content)).metadata?.tags ?? [];
      return query.tags?.every(tag => tags.includes(tag)) ?? true;
    }).map(([path, file]) => headEntry(path, file));
    items.sort((a, b) => a.path.localeCompare(b.path));
    const start = query.cursor ? items.findIndex(item => item.path === query.cursor) + 1 : 0;
    const page = items.slice(start, start + (query.limit ?? 500));
    return {
      items: page, next_cursor: start + page.length < items.length ? page.at(-1)?.path ?? null : null,
      source: query.source ?? "store", consistency: "per_file",
    };
  },
  async readRaw(path: string): Promise<{ bytes: Uint8Array; etag: string }> {
    await latency();
    const top = head(requireLive(path));
    return { bytes: top.content!.bytes.slice(), etag: top.revision };
  },
  async readText(path: string): Promise<{ text: string; etag: string }> {
    const { bytes, etag } = await this.readRaw(path);
    return { text: new TextDecoder().decode(bytes), etag };
  },
  async readMeta(path: string): Promise<Entry> {
    await latency();
    return headEntry(path, requireLive(path));
  },
  async saveText(path: string, body: string, precondition: Precondition, _operationId?: string): Promise<MutationResult> {
    return putContent(path, body, precondition, "text/markdown; charset=utf-8");
  },
  async saveBinary(path: string, body: ArrayBuffer, precondition: Precondition, mediaType: string): Promise<MutationResult> {
    return putContent(path, body, precondition, mediaType);
  },
  async trash(path: string, baseRevision: string): Promise<MutationResult> {
    await latency();
    checkRevision(requireLive(path), baseRevision);
    return commit(path, "trash");
  },
  async purge(path: string, baseRevision: string): Promise<{ path: string; purged: boolean; index_state: string; operation_id: string }> {
    await latency();
    const file = requireFile(path);
    if (head(file).action !== "trash") throw new ApiError(409, "PURGE_REQUIRES_TRASHED", `${path} 仍是 live 状态,先移入回收站`);
    checkRevision(file, baseRevision);
    store.delete(path);
    return { path, purged: true, index_state: "ready", operation_id: nextRevision() };
  },
  async history(path: string, _cursor?: string | null): Promise<HistoryPage> {
    await latency();
    const file = requireFile(path);
    return {
      items: file.revisions.map((rev) => ({
        revision: rev.revision, path, generation: rev.generation, parent: rev.parent,
        action: rev.action, actor: "demo", created_at: rev.created_at,
        content: rev.content ? {
          size: rev.content.bytes.byteLength, sha256: rev.content.sha256,
          media_type: rev.content.media_type, executable: false,
        } : undefined,
        commit_state: "reachable" as const,
      })),
      anchor_revision: head(file).revision, next_cursor: null,
    };
  },
  async readRevisionText(path: string, revision: string): Promise<string> {
    return new TextDecoder().decode(await this.readRevisionRaw(path, revision));
  },
  async readRevisionRaw(path: string, revision: string): Promise<Uint8Array> {
    await latency();
    const found = requireFile(path).revisions.find(rev => rev.revision === revision);
    if (!found) throw new ApiError(404, "NOT_FOUND", "版本不存在");
    if (!found.content) throw new ApiError(410, "GONE", "该版本没有内容");
    return found.content.bytes.slice();
  },
  async restore(path: string, revision: string, baseRevision: string): Promise<MutationResult> {
    await latency();
    const file = requireFile(path);
    checkRevision(file, baseRevision);
    const source = file.revisions.find(rev => rev.revision === revision);
    if (!source?.content) throw new ApiError(400, "INVALID_ARGUMENT", "该版本没有内容");
    return commit(path, "restore", source.content);
  },
  async search(query: { q: string; tags?: string[]; mode: "literal" | "fts"; prefix?: string; cursor?: string | null }): Promise<SearchPage> {
    await latency();
    const q = query.q.trim();
    const items: SearchPage["items"] = [];
    for (const [path, file] of store) {
      if (head(file).action === "trash" || !path.startsWith(query.prefix ?? "")) continue;
      const text = textOf(head(file).content);
      const metadata = parseMarkdownDocument(text).metadata;
      if (query.tags?.some(tag => !metadata?.tags.includes(tag))) continue;
      const haystack = `${path}\n${metadata?.title ?? ""}\n${text}`;
      const hit = query.mode === "literal" ? haystack.includes(q)
        : q.split(/\s+/).filter(Boolean).every(token => haystack.toLowerCase().includes(token.toLowerCase()));
      if (q && hit) {
        const index = Math.max(0, text.toLowerCase().indexOf(q.toLowerCase()));
        const start = Math.max(0, index - 30);
        items.push({ path, indexed_revision: head(file).revision,
          snippet: `${start > 0 ? "…" : ""}${text.slice(start, index + q.length + 60)}` });
      }
    }
    return { items, next_cursor: null, complete: true, scanned: store.size, index_state: "available" };
  },
  async links(path: string, direction: "outbound" | "backlinks"): Promise<{ items: LinkItem[] }> {
    await latency();
    const live = [...store].filter(([, file]) => head(file).action !== "trash");
    const files = live.map(([path]) => ({ path }));
    const items: LinkItem[] = [];
    for (const [source, file] of live) {
      const doc = parseMarkdownDocument(textOf(head(file).content));
      const parser = createMarkdownParser();
      const targets = new Set(doc.metadata?.links.map(link => link.target) ?? []);
      parser.walkTokens(parser.lexer(doc.body), (token) => {
        if (token.type === "link" || token.type === "image") targets.add(token.href);
      });
      const resolved = new Set<string>();
      for (const target of targets) {
        const ref = resolveReference(target, files, source);
        if (ref.kind !== "vault" || resolved.has(ref.path)) continue;
        resolved.add(ref.path);
        if ((direction === "outbound" && source === path) || (direction === "backlinks" && ref.path === path)) {
          items.push({ source, target: ref.path, kind: "wiki", source_revision: head(file).revision });
        }
      }
    }
    return { items };
  },
  async copy(source: string, target: string, expectedSourceRevision: string): Promise<TransferResult> {
    return transfer(source, target, expectedSourceRevision, "copy");
  },
  async move(source: string, target: string, expectedSourceRevision: string): Promise<TransferResult> {
    return transfer(source, target, expectedSourceRevision, "move");
  },
};
