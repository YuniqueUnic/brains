/** Upload conflict planning: pure functions, no DOM (inkmark's
 * src/core/import-conflict.ts adapted to Brains' vault model).
 *
 * Conflict key = the target vault path. `direct` when free; `identical`
 * when an entry exists with the same content hash (fixed skip); otherwise
 * `rename` is the default decision. Rename targets increment `-N` before the
 * extension, avoiding both existing entries and the other rows' landings.
 * The diagnostic tree only flips actions; the executor performs whatever the
 * final rows say — cancel means nothing lands at all.
 */

export type UploadAction = "direct" | "rename" | "overwrite" | "skip" | "identical";

export interface IncomingFile {
  /** Target vault path (folder uploads keep their relative path). */
  path: string;
  name: string;
  text: string | null;
  bytes: ArrayBuffer | null;
  mediaType: string;
  sha256: string;
}

export interface ExistingEntry {
  path: string;
  revision: string;
  sha256?: string;
}

export interface UploadRow {
  incoming: IncomingFile;
  action: UploadAction;
  /** The colliding entry (undefined on direct rows). */
  existing?: ExistingEntry;
  /** Rename rows: the free target path. */
  renameTo?: string;
}

export interface UploadPlan {
  rows: UploadRow[];
  /** Any decision-needed row — decides whether the dialog shows. */
  hasConflicts: boolean;
  identicalCount: number;
  directCount: number;
}

/** Initial plan: free → direct; same content → identical; else → rename.
 * Two files in one batch claiming the same path also collide: the second is
 * auto-renamed — unless its content is byte-identical to the first, in which
 * case it is plain `identical` (a rename would land a pointless copy that
 * the conflict tree never showed). */
export function buildUploadPlan(incoming: IncomingFile[], existing: ExistingEntry[]): UploadPlan {
  const byPath = new Map(existing.map((entry) => [entry.path, entry]));
  const claimed = new Map<string, string>();
  const rows = incoming.map((file): UploadRow => {
    const hit = byPath.get(file.path);
    if (hit) {
      if (hit.sha256 && hit.sha256 === file.sha256) {
        claimed.set(file.path, file.sha256);
        return { incoming: file, action: "identical", existing: hit };
      }
      claimed.set(file.path, file.sha256);
      return { incoming: file, action: "rename", existing: hit };
    }
    const claimedSha = claimed.get(file.path);
    if (claimedSha !== undefined) {
      if (claimedSha === file.sha256) {
        return { incoming: file, action: "identical" };
      }
      return { incoming: file, action: "rename" };
    }
    claimed.set(file.path, file.sha256);
    return { incoming: file, action: "direct" };
  });
  return finalize(rows, existing);
}

/** Flip one row's action; rename landings are re-planned afterwards. */
export function setRowAction(
  rows: UploadRow[],
  existing: ExistingEntry[],
  index: number,
  action: UploadAction,
): UploadRow[] {
  return finalize(
    rows.map((row, i) => (i === index ? { ...row, action, renameTo: undefined } : row)),
    existing,
  ).rows;
}

/** Preset from the dialog header: every conflicted row becomes overwrite. */
export function overwriteAll(rows: UploadRow[], existing: ExistingEntry[]): UploadRow[] {
  return finalize(
    rows.map((row) =>
      row.existing && row.action !== "identical"
        ? { ...row, action: "overwrite" as const, renameTo: undefined }
        : row,
    ),
    existing,
  ).rows;
}

/** Assign rename targets in row order, avoiding existing + sibling landings. */
function finalize(rows: UploadRow[], existing: ExistingEntry[]): UploadPlan {
  const taken = new Set<string>(existing.map((entry) => entry.path));
  for (const row of rows) {
    if (row.action !== "rename") taken.add(row.incoming.path);
  }
  const out = rows.map((row): UploadRow => {
    if (row.action !== "rename") return row;
    const target = renameTarget(row.incoming.path, taken);
    taken.add(target);
    return { ...row, renameTo: target };
  });
  return {
    rows: out,
    hasConflicts: out.some(
      (row) => row.action === "rename" || row.action === "overwrite" || row.action === "skip",
    ),
    identicalCount: out.filter((row) => row.action === "identical").length,
    directCount: out.filter((row) => row.action === "direct").length,
  };
}

/** notes/a.md → notes/a-1.md → notes/a-2.md … (whole stem when no dot). */
function renameTarget(path: string, taken: Set<string>): string {
  const slash = path.lastIndexOf("/");
  const dir = slash >= 0 ? path.slice(0, slash + 1) : "";
  const name = slash >= 0 ? path.slice(slash + 1) : path;
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const ext = dot > 0 ? name.slice(dot) : "";
  for (let i = 1; ; i += 1) {
    const candidate = `${dir}${stem}-${i}${ext}`;
    if (!taken.has(candidate)) return candidate;
  }
}
