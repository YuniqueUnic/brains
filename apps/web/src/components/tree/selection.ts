/*
 * Batch-selection model (DESIGN.md §9-15): the selection set holds FILE
 * paths only. A folder row's checkbox is derived from the files beneath it —
 * all / partial / none — so nested folders light up automatically and a
 * single deselected file turns every ancestor partial. Deriving instead of
 * storing folder rows kills the whole class of "folder + members" double
 * representation (deselect-one-child-under-a-selected-folder).
 */

export type CheckState = "all" | "partial" | "none";

/** Every file beneath `folder` (exclusive — the folder itself is not a file). */
function filesUnder(files: string[], folder: string): string[] {
  const prefix = `${folder}/`;
  return files.filter((file) => file.startsWith(prefix));
}

/** Tri-state for a tree row: files are binary; folders summarize their subtree. */
export function checkState(
  files: string[],
  path: string,
  isFolder: boolean,
  selected: Set<string>,
): CheckState {
  if (!isFolder) return selected.has(path) ? "all" : "none";
  const group = filesUnder(files, path);
  if (group.length === 0) return "none";
  const hits = group.filter((file) => selected.has(file)).length;
  if (hits === 0) return "none";
  return hits === group.length ? "all" : "partial";
}

/** Clicking a folder checks / unchecks every file beneath it. */
export function togglePath(
  files: string[],
  path: string,
  isFolder: boolean,
  selected: Set<string>,
): Set<string> {
  const group = isFolder ? filesUnder(files, path) : [path];
  const next = new Set(selected);
  const allSelected = group.every((file) => next.has(file));
  for (const file of group) {
    if (allSelected) next.delete(file);
    else next.add(file);
  }
  return next;
}

export function selectAllFiles(files: string[]): Set<string> {
  return new Set(files);
}

/** Invert over files; a folder row ends up checked/partial purely by derivation. */
export function invertSelection(files: string[], selected: Set<string>): Set<string> {
  return new Set(files.filter((file) => !selected.has(file)));
}

/**
 * Drag payload of "row + selection": entries that already ride inside another
 * entry's folder are dropped, so dragging a folder that is also batch-selected
 * moves it once instead of once per descendant.
 */
export function collapseToRoots<T extends { path: string; isFolder: boolean }>(
  entries: T[],
): T[] {
  // Folder payloads carry a trailing slash ("docs/"); compare plain paths.
  const plain = (entry: T) => (entry.isFolder ? entry.path.replace(/\/+$/, "") : entry.path);
  return entries.filter((entry) => {
    const path = plain(entry);
    return !entries.some((other) => {
      if (other === entry || !other.isFolder) return false;
      const folder = plain(other);
      return folder !== "" && path.startsWith(`${folder}/`);
    });
  });
}
