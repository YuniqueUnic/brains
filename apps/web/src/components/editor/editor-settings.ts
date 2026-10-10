/*
 * Editor preferences (fonts are independent of the UI font) and unsaved-
 * buffer drafts, both persisted to localStorage. Preferences apply through
 * compartments so the view never rebuilds; every dirty buffer is debounce-
 * persisted keyed by path — reopening the file adopts the draft as dirty
 * content with a discard action. Cleared when the buffer returns to clean
 * (save, revert, take remote). Best-effort only — quota errors are swallowed.
 */

export interface EditorSettings {
  lineNumbers: boolean;
  wrap: boolean;
  font: "sans" | "serif" | "mono";
  /** Percent, 100 = the 15px document base. */
  scale: number;
  /** The markdown quick-format row under the header. */
  toolbar: boolean;
}

export const EDITOR_DEFAULTS: EditorSettings = {
  lineNumbers: false,
  wrap: true,
  font: "sans",
  scale: 100,
  toolbar: true,
};

const EDITOR_KEY = "brains-editor";

export function loadEditorSettings(): EditorSettings {
  try {
    const raw = localStorage.getItem(EDITOR_KEY);
    if (raw) return { ...EDITOR_DEFAULTS, ...(JSON.parse(raw) as Partial<EditorSettings>) };
  } catch {
    /* ignore malformed state */
  }
  return EDITOR_DEFAULTS;
}

export function saveEditorSettings(settings: EditorSettings) {
  try {
    localStorage.setItem(EDITOR_KEY, JSON.stringify(settings));
  } catch {
    /* best-effort */
  }
}

/* --- Drafts ---------------------------------------------------------------- */

const DRAFT_PREFIX = "brains-draft:";
export const DRAFT_LIMIT = 500_000;

interface EditorDraft {
  text: string;
  at: number;
}

const draftPending = new Map<string, { text: string; timer: number }>();

export function writeDraft(path: string, text: string) {
  const pending = draftPending.get(path);
  if (pending) window.clearTimeout(pending.timer);
  const timer = window.setTimeout(() => {
    draftPending.delete(path);
    try {
      if (text.length <= DRAFT_LIMIT) {
        localStorage.setItem(
          DRAFT_PREFIX + path,
          JSON.stringify({ text, at: Date.now() } satisfies EditorDraft),
        );
      }
    } catch {
      /* quota or unavailable — drafts are best-effort */
    }
  }, 600);
  draftPending.set(path, { text, timer });
}

export function clearDraft(path: string) {
  const pending = draftPending.get(path);
  if (pending) {
    window.clearTimeout(pending.timer);
    draftPending.delete(path);
  }
  try {
    localStorage.removeItem(DRAFT_PREFIX + path);
  } catch {
    /* ignore */
  }
}

export function readDraft(path: string): EditorDraft | null {
  try {
    const raw = localStorage.getItem(DRAFT_PREFIX + path);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as EditorDraft;
    return typeof parsed?.text === "string" && typeof parsed?.at === "number" ? parsed : null;
  } catch {
    return null;
  }
}

/** Flush debounce-pending drafts synchronously (unload / tab hide). */
export function flushDrafts() {
  for (const [path, pending] of draftPending) {
    window.clearTimeout(pending.timer);
    try {
      if (pending.text.length <= DRAFT_LIMIT) {
        localStorage.setItem(
          DRAFT_PREFIX + path,
          JSON.stringify({ text: pending.text, at: Date.now() } satisfies EditorDraft),
        );
      }
    } catch {
      /* ignore */
    }
  }
  draftPending.clear();
}
