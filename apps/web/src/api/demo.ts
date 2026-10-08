import {
  ApiError,
  isTextMedia,
  type Entry,
  type FilePage,
  type HistoryPage,
  type MutationResult,
  type Precondition,
  type SearchPage,
  type SessionInfo,
  type TransferResult,
} from "./client";

/*
 * In-memory demo backend for the GitHub Pages demo (no server): same wire
 * semantics as the real HTTP client — conditional writes with 412 on stale
 * bases, tombstone trash, restore-as-new-revision, literal/FTS search and
 * [[link]] resolution — so the demo exercises the real UI logic.
 */

interface DemoContent {
  size: number;
  sha256: string;
  media_type: string;
  text?: string;
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
let generationCounter = 0;

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function latency() {
  return sleep(60 + Math.random() * 120);
}

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

function nextRevision(): string {
  revisionCounter += 1;
  return revisionCounter.toString(16).padStart(8, "0") + "demo";
}

function head(file: DemoFile): DemoRevision {
  return file.revisions[0];
}

function headEntry(path: string, file: DemoFile): Entry {
  const top = head(file);
  return {
    path,
    revision: top.revision,
    generation: top.generation,
    state: top.action === "trash" ? "trashed" : "live",
    updated_at: top.created_at,
    size: top.content?.size,
    sha256: top.content?.sha256,
    media_type: top.content?.media_type,
    kind: top.content && isTextMedia(top.content.media_type) ? "note" : "binary",
    title: undefined,
  };
}

function mutation(path: string, file: DemoFile): MutationResult {
  const entry = headEntry(path, file);
  return { ...entry, index_state: "ready", operation_id: nextRevision() };
}

function requireLive(path: string): DemoFile {
  const file = store.get(path);
  if (!file || head(file).action === "trash") {
    throw new ApiError(410, "GONE", `${path} 不存在（或已在回收站）`);
  }
  return file;
}

async function putContent(
  path: string,
  body: string | ArrayBuffer,
  precondition: Precondition,
  mediaType: string,
): Promise<MutationResult> {
  await latency();
  const existing = store.get(path);
  const live = existing && head(existing).action !== "trash" ? existing : undefined;
  const text = typeof body === "string" ? body : new TextDecoder().decode(body);
  const content: DemoContent = {
    size: text.length,
    sha256: await sha256Hex(text),
    media_type: mediaType,
    text,
  };
  if (precondition.mode === "create") {
    if (live) {
      throw new ApiError(412, "ALREADY_EXISTS", `${path} 已存在`, head(live).revision);
    }
  } else {
    if (!live) {
      throw new ApiError(404, "NOT_FOUND", `${path} 不存在`);
    }
    if (head(live).revision !== precondition.baseRevision) {
      throw new ApiError(
        412,
        "REVISION_CONFLICT",
        "基线已过期：远端已有更新版本",
        head(live).revision,
      );
    }
  }
  const revision: DemoRevision = {
    revision: nextRevision(),
    generation: live ? head(live).generation + 1 : ++generationCounter,
    parent: live ? head(live).revision : null,
    action: "put",
    created_at: new Date().toISOString(),
    content,
  };
  const file: DemoFile = { revisions: [revision, ...(live?.revisions ?? [])] };
  store.set(path, file);
  return mutation(path, file);
}

function seed() {
  for (const item of SEEDS) {
    const text = item.text;
    const revision: DemoRevision = {
      revision: nextRevision(),
      generation: ++generationCounter,
      parent: null,
      action: "put",
      created_at: new Date().toISOString(),
      content: {
        size: text.length,
        sha256: "",
        media_type: item.mediaType,
        text,
      },
    };
    store.set(item.path, { revisions: [revision] });
  }
}
seed();

const session: SessionInfo = {
  actor: "demo",
  scopes: ["read:/", "write:/", "admin:/"],
  expires_at: undefined,
  csrf_token: undefined,
};

/** Same surface as the HTTP api object; swapped in when running as a demo. */
export const demoApi = {
  async createSession(_token: string): Promise<SessionInfo> {
    await latency();
    return session;
  },
  async getSession(): Promise<SessionInfo> {
    return session;
  },
  async deleteSession(): Promise<void> {
    await latency();
  },

  async listFiles(query: {
    prefix?: string;
    cursor?: string | null;
    limit?: number;
    include_deleted?: boolean;
    source?: "store" | "index";
    tag?: string;
    sort?: string;
  }): Promise<FilePage> {
    await latency();
    const items: Entry[] = [];
    for (const [path, file] of store) {
      const trashed = head(file).action === "trash";
      if (trashed && !query.include_deleted) continue;
      items.push(headEntry(path, file));
    }
    items.sort((a, b) => a.path.localeCompare(b.path));
    return { items, next_cursor: null, source: "store", consistency: "per_file" };
  },

  async readRaw(path: string): Promise<{ bytes: Uint8Array; etag: string }> {
    await latency();
    const file = requireLive(path);
    const text = head(file).content?.text ?? "";
    return { bytes: new TextEncoder().encode(text), etag: head(file).revision };
  },

  async readText(path: string): Promise<{ text: string; etag: string }> {
    const { bytes, etag } = await this.readRaw(path);
    return { text: new TextDecoder().decode(bytes), etag };
  },

  async readMeta(path: string): Promise<Entry> {
    await latency();
    const file = requireLive(path);
    return headEntry(path, file);
  },

  async saveText(
    path: string,
    body: string,
    precondition: Precondition,
    _operationId?: string,
  ): Promise<MutationResult> {
    return putContent(path, body, precondition, "text/markdown; charset=utf-8");
  },

  async saveBinary(
    path: string,
    body: ArrayBuffer,
    precondition: Precondition,
    mediaType: string,
  ): Promise<MutationResult> {
    return putContent(path, body, precondition, mediaType);
  },

  async trash(path: string, baseRevision: string): Promise<MutationResult> {
    await latency();
    const file = requireLive(path);
    if (head(file).revision !== baseRevision) {
      throw new ApiError(412, "REVISION_CONFLICT", "基线已过期", head(file).revision);
    }
    const revision: DemoRevision = {
      revision: nextRevision(),
      generation: head(file).generation + 1,
      parent: head(file).revision,
      action: "trash",
      created_at: new Date().toISOString(),
    };
    file.revisions = [revision, ...file.revisions];
    return mutation(path, file);
  },

  async history(path: string, _cursor?: string | null): Promise<HistoryPage> {
    await latency();
    const file = store.get(path);
    if (!file) throw new ApiError(404, "NOT_FOUND", `${path} 不存在`);
    return {
      items: file.revisions.map((rev) => ({
        revision: rev.revision,
        path,
        generation: rev.generation,
        parent: rev.parent,
        action: rev.action,
        actor: "demo",
        operation_id: undefined,
        created_at: rev.created_at,
        content: rev.content
          ? {
              size: rev.content.size,
              sha256: rev.content.sha256,
              media_type: rev.content.media_type,
              executable: false,
            }
          : undefined,
        commit_state: "reachable" as const,
      })),
      anchor_revision: head(file).revision,
      next_cursor: null,
    };
  },

  async readRevisionText(path: string, revision: string): Promise<string> {
    await latency();
    const file = store.get(path);
    const found = file?.revisions.find((rev) => rev.revision === revision);
    return found?.content?.text ?? "";
  },

  async restore(
    path: string,
    revision: string,
    baseRevision: string,
  ): Promise<MutationResult> {
    await latency();
    const file = store.get(path);
    if (!file) throw new ApiError(404, "NOT_FOUND", `${path} 不存在`);
    if (head(file).revision !== baseRevision) {
      throw new ApiError(412, "REVISION_CONFLICT", "基线已过期", head(file).revision);
    }
    const source = file.revisions.find((rev) => rev.revision === revision);
    if (!source?.content) throw new ApiError(400, "INVALID_ARGUMENT", "该版本没有内容");
    const restored: DemoRevision = {
      revision: nextRevision(),
      generation: head(file).generation + 1,
      parent: head(file).revision,
      action: "restore",
      created_at: new Date().toISOString(),
      content: source.content,
    };
    file.revisions = [restored, ...file.revisions];
    return mutation(path, file);
  },

  async search(query: {
    q: string;
    mode: "literal" | "fts";
    prefix?: string;
    cursor?: string | null;
  }): Promise<SearchPage> {
    await latency();
    const q = query.q.trim();
    const items: import("./client").SearchHit[] = [];
    for (const [path, file] of store) {
      if (head(file).action === "trash") continue;
      const text = head(file).content?.text ?? "";
      const hit =
        query.mode === "literal"
          ? text.includes(q)
          : q
              .split(/\s+/)
              .filter(Boolean)
              .every((token) => text.toLowerCase().includes(token.toLowerCase()));
      if (q && hit) {
        const index = text.indexOf(q);
        const start = Math.max(0, index - 30);
        items.push({
          path,
          indexed_revision: head(file).revision,
          snippet: `${start > 0 ? "…" : ""}${text.slice(start, index + q.length + 60)}`,
        });
      }
    }
    return { items, next_cursor: null, complete: true, scanned: store.size, index_state: "available" };
  },

  async links(
    path: string,
    direction: "outbound" | "backlinks",
  ): Promise<{ items: { source: string; target: string; kind: string }[] }> {
    await latency();
    const items: { source: string; target: string; kind: string }[] = [];
    const wikiLinks = (text: string) =>
      [...text.matchAll(/\[\[([^\]]+)\]\]/g)].map((match) => match[1]);
    const resolve = (name: string) =>
      [...store.keys()].find((key) => key.endsWith(`/${name}`) || key === name || key.endsWith(name));
    if (direction === "outbound") {
      const file = store.get(path);
      const text = file?.revisions.find((rev) => rev.content)?.content?.text ?? "";
      for (const name of wikiLinks(text)) {
        const target = resolve(name);
        if (target) items.push({ source: path, target, kind: "wiki" });
      }
    } else {
      const stem = path.split("/").pop() ?? path;
      for (const [candidate, file] of store) {
        if (candidate === path) continue;
        const text = file.revisions.find((rev) => rev.content)?.content?.text ?? "";
        if (wikiLinks(text).some((name) => name === stem || stem.endsWith(name))) {
          items.push({ source: candidate, target: path, kind: "wiki" });
        }
      }
    }
    return { items };
  },

  async copy(
    source: string,
    target: string,
    expectedSourceRevision: string,
  ): Promise<TransferResult> {
    await latency();
    const file = requireLive(source);
    if (head(file).revision !== expectedSourceRevision) {
      throw new ApiError(412, "REVISION_CONFLICT", "基线已过期", head(file).revision);
    }
    const content = head(file).content;
    const operationId = nextRevision();
    if (store.has(target) && head(store.get(target)!).action !== "trash") {
      return {
        copied: false,
        source_trashed: false,
        partial: false,
        operation_id: operationId,
        error: { code: "ALREADY_EXISTS", message: `${target} 已存在` },
      };
    }
    await putContent(target, content?.text ?? "", { mode: "create" }, content?.media_type ?? "text/markdown");
    return { copied: true, source_trashed: false, partial: false, operation_id: operationId, target_revision: head(store.get(target)!).revision };
  },

  async move(
    source: string,
    target: string,
    expectedSourceRevision: string,
  ): Promise<TransferResult> {
    await latency();
    const file = requireLive(source);
    if (head(file).revision !== expectedSourceRevision) {
      throw new ApiError(412, "REVISION_CONFLICT", "基线已过期", head(file).revision);
    }
    const operationId = nextRevision();
    if (store.has(target) && head(store.get(target)!).action !== "trash") {
      return {
        copied: false,
        source_trashed: false,
        partial: false,
        operation_id: operationId,
        error: { code: "ALREADY_EXISTS", message: `${target} 已存在` },
      };
    }
    const content = head(file).content;
    const result = await putContent(target, content?.text ?? "", { mode: "create" }, content?.media_type ?? "text/markdown");
    await this.trash(source, head(file).revision);
    return {
      copied: true,
      source_trashed: true,
      partial: false,
      operation_id: operationId,
      target_revision: result.revision,
      source_revision: head(file).revision,
    };
  },
};
