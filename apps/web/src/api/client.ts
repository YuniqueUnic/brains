import { demoApi } from "./demo";

/**
 * HTTP transport for the web app (design §22.4): one module owns URLs,
 * conditional headers, CSRF and error parsing. Every mutation carries an
 * explicit revision precondition; nothing reads-and-overwrites silently.
 */

export type Precondition = { mode: "create" } | { mode: "update"; baseRevision: string };

export interface Entry {
  path: string;
  revision: string;
  generation: number;
  state: "live" | "trashed";
  updated_at?: string;
  size?: number;
  sha256?: string;
  media_type?: string;
  kind?: "note" | "text" | "script" | "binary";
  title?: string;
}

export interface MutationResult extends Entry {
  index_state: "ready" | "pending" | "disabled";
  operation_id: string;
}

export interface FilePage {
  items: Entry[];
  next_cursor: string | null;
  source: "store" | "index";
  consistency: "per_file";
}

export interface RevisionDto {
  revision: string;
  path: string;
  generation: number;
  parent: string | null;
  action: "put" | "trash" | "restore" | "copy";
  actor?: string;
  created_at: string;
  content?: { size: number; sha256: string; media_type: string; executable: boolean };
  restores_revision?: string;
  commit_state?: "reachable" | "unknown";
}

export interface HistoryPage {
  items: RevisionDto[];
  anchor_revision: string;
  next_cursor: string | null;
}

export interface SearchHit {
  path: string;
  indexed_revision: string;
  snippet: string;
}

export interface SearchPage {
  items: SearchHit[];
  next_cursor: string | null;
  complete: boolean;
  scanned: number;
  index_state: "available" | "building";
}

export interface TransferResult {
  copied: boolean;
  source_trashed: boolean;
  partial: boolean;
  operation_id: string;
  target_revision?: string;
  source_revision?: string;
  error?: { code: string; message: string; current_revision?: string };
}

export interface LinkItem {
  source: string;
  target: string;
  kind: string;
  source_revision?: string;
}

export interface SessionInfo {
  actor: string;
  scopes: string[];
  expires_at?: string;
  csrf_token?: string;
}

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly currentRevision?: string;
  constructor(status: number, code: string, message: string, currentRevision?: string) {
    super(message);
    this.status = status;
    this.code = code;
    this.currentRevision = currentRevision;
  }
}

function randomOperationId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

let csrfToken: string | null = null;

export function setCsrfToken(token: string | null): void {
  csrfToken = token;
}

async function request<T>(path: string, init: RequestInit): Promise<T> {
  const headers = new Headers(init.headers);
  const method = (init.method ?? "GET").toUpperCase();
  if (csrfToken && method !== "GET" && method !== "HEAD") {
    headers.set("X-CSRF-Token", csrfToken);
  }
  const response = await fetch(`/api/v1${path}`, { ...init, headers, credentials: "same-origin" });
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  let body: unknown = undefined;
  try {
    body = text ? JSON.parse(text) : undefined;
  } catch {
    body = undefined;
  }
  if (!response.ok) {
    const detail = (body as { error?: { code?: string; message?: string; current_revision?: string } })
      ?.error;
    throw new ApiError(
      response.status,
      detail?.code ?? "UNKNOWN",
      detail?.message ?? `HTTP ${response.status}`,
      detail?.current_revision,
    );
  }
  return body as T;
}

export function isTextMedia(mediaType?: string): boolean {
  const base = mediaType?.split(";")[0]?.trim() ?? "";
  return (
    base.startsWith("text/") ||
    base === "application/json" ||
    base === "application/yaml" ||
    base === "application/toml"
  );
}

const httpApi = {
  async createSession(token: string): Promise<SessionInfo> {
    const session = await request<SessionInfo>("/session", {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    });
    setCsrfToken(session.csrf_token ?? null);
    return session;
  },

  async getSession(): Promise<SessionInfo> {
    const session = await request<SessionInfo>("/session", {});
    setCsrfToken(session.csrf_token ?? null);
    return session;
  },

  async deleteSession(): Promise<void> {
    await request<{ ok: true }>("/session", { method: "DELETE" });
    setCsrfToken(null);
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
    const params = new URLSearchParams();
    if (query.prefix) params.set("prefix", query.prefix);
    if (query.cursor) params.set("cursor", query.cursor);
    if (query.limit) params.set("limit", String(query.limit));
    if (query.include_deleted) params.set("include_deleted", "true");
    if (query.source) params.set("source", query.source);
    if (query.tag) params.set("tag", query.tag);
    if (query.sort) params.set("sort", query.sort);
    return request<FilePage>(`/files?${params}`, {});
  },

  async readRaw(path: string): Promise<{ bytes: Uint8Array; etag: string }> {
    const response = await fetch(`/api/v1/file?path=${encodeURIComponent(path)}`, {
      credentials: "same-origin",
    });
    if (!response.ok) throw await errorFrom(response);
    const etag = response.headers.get("ETag") ?? "";
    return { bytes: new Uint8Array(await response.arrayBuffer()), etag: etag.replace(/"/g, "") };
  },

  async readText(path: string): Promise<{ text: string; etag: string }> {
    const { bytes, etag } = await this.readRaw(path);
    return { text: new TextDecoder().decode(bytes), etag };
  },

  async readMeta(path: string): Promise<Entry> {
    return request<Entry>(`/file?path=${encodeURIComponent(path)}&format=meta`, {});
  },

  /**
   * The single save path (design §13.2): one save in flight, explicit
   * precondition, operation id kept for lost-response forensics.
   */
  async saveText(
    path: string,
    body: string,
    precondition: Precondition,
    operationId: string = randomOperationId(),
  ): Promise<MutationResult> {
    const headers: Record<string, string> = {
      "Content-Type": "text/markdown; charset=utf-8",
      "X-Operation-ID": operationId,
    };
    if (precondition.mode === "create") {
      headers["If-None-Match"] = "*";
    } else {
      headers["If-Match"] = `"${precondition.baseRevision}"`;
    }
    return request<MutationResult>(`/file?path=${encodeURIComponent(path)}`, {
      method: "PUT",
      headers,
      body,
    });
  },

  async saveBinary(
    path: string,
    body: ArrayBuffer,
    precondition: Precondition,
    mediaType: string,
    operationId: string = randomOperationId(),
  ): Promise<MutationResult> {
    const digest = await sha256Hex(body);
    const headers: Record<string, string> = {
      "Content-Type": mediaType,
      "X-Content-SHA256": digest,
      "X-Operation-ID": operationId,
    };
    if (precondition.mode === "create") headers["If-None-Match"] = "*";
    else headers["If-Match"] = `"${precondition.baseRevision}"`;
    return request<MutationResult>(`/file?path=${encodeURIComponent(path)}`, {
      method: "PUT",
      headers,
      body,
    });
  },

  async trash(
    path: string,
    baseRevision: string,
    operationId: string = randomOperationId(),
  ): Promise<MutationResult> {
    return request<MutationResult>(`/file?path=${encodeURIComponent(path)}`, {
      method: "DELETE",
      headers: { "If-Match": `"${baseRevision}"`, "X-Operation-ID": operationId },
    });
  },

  async history(path: string, cursor?: string | null): Promise<HistoryPage> {
    const params = new URLSearchParams({ path });
    if (cursor) params.set("cursor", cursor);
    return request<HistoryPage>(`/revisions?${params}`, {});
  },

  async readRevisionText(path: string, revision: string): Promise<string> {
    const response = await fetch(
      `/api/v1/revision?path=${encodeURIComponent(path)}&revision=${revision}`,
      { credentials: "same-origin" },
    );
    if (!response.ok) throw await errorFrom(response);
    return new TextDecoder().decode(await response.arrayBuffer());
  },

  async restore(
    path: string,
    revision: string,
    baseRevision: string,
    operationId: string = randomOperationId(),
  ): Promise<MutationResult> {
    return request<MutationResult>("/restore", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "If-Match": `"${baseRevision}"`,
        "X-Operation-ID": operationId,
      },
      body: JSON.stringify({ path, revision }),
    });
  },

  async search(query: {
    q: string;
    mode: "literal" | "fts";
    prefix?: string;
    cursor?: string | null;
  }): Promise<SearchPage> {
    const params = new URLSearchParams({ q: query.q, mode: query.mode });
    if (query.prefix) params.set("prefix", query.prefix);
    if (query.cursor) params.set("cursor", query.cursor);
    return request<SearchPage>(`/search?${params}`, {});
  },

  async links(path: string, direction: "outbound" | "backlinks"): Promise<{ items: LinkItem[] }> {
    return request(`/links?path=${encodeURIComponent(path)}&direction=${direction}`, {});
  },

  /** Server-side copy (duplicate): the source keeps its revision. */
  async copy(
    source: string,
    target: string,
    expectedSourceRevision: string,
  ): Promise<TransferResult> {
    return request<TransferResult>("/copy", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        source,
        target,
        expected_source_revision: expectedSourceRevision,
        operation_id: randomOperationId(),
      }),
    });
  },

  /** Server-side move — rename is a move whose target path differs. */
  async move(
    source: string,
    target: string,
    expectedSourceRevision: string,
  ): Promise<TransferResult> {
    return request<TransferResult>("/move", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        source,
        target,
        expected_source_revision: expectedSourceRevision,
        operation_id: randomOperationId(),
      }),
    });
  },
};

async function errorFrom(response: Response): Promise<ApiError> {
  let detail: { code?: string; message?: string; current_revision?: string } = {};
  try {
    detail = (await response.json())?.error ?? {};
  } catch {
    /* non-JSON error body */
  }
  return new ApiError(
    response.status,
    detail.code ?? "UNKNOWN",
    detail.message ?? `HTTP ${response.status}`,
    detail.current_revision,
  );
}

/**
 * The app talks to this one object. When built for the GitHub Pages demo
 * (VITE_DEMO_MODE=1) it is the in-memory backend with identical wire
 * semantics; otherwise it is the real HTTP client.
 */
const DEMO =
  (import.meta.env?.VITE_DEMO_MODE as string | undefined) === "1" ||
  window.location.hostname.endsWith(".github.io");

export const api: typeof httpApi = DEMO ? (demoApi as typeof httpApi) : httpApi;

async function sha256Hex(buffer: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}
