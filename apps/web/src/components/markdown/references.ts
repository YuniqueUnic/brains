export type Reference =
  | { kind: "vault"; path: string; fragment: string }
  | { kind: "external"; url: string }
  | { kind: "anchor"; fragment: string }
  | { kind: "missing"; target: string };

export const referenceStem = (path: string) =>
  path.replace(/\.[a-z0-9]+$/i, "");

function normalize(path: string): string | null {
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (part === "..") {
      if (!parts.length) return null;
      parts.pop();
    } else if (part && part !== ".") parts.push(part);
  }
  return parts.join("/");
}

/** Relative paths are resolved from the document; ambiguous short names never pick an arbitrary file. */
export function resolveReference(
  target: string,
  files: readonly { path: string }[],
  source = "",
): Reference {
  const raw = target.trim().replace(/^<|>$/g, "");
  if (/^(https?:|mailto:)/i.test(raw)) return { kind: "external", url: raw };
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw) || raw.startsWith("//"))
    return { kind: "missing", target };
  if (raw.startsWith("#")) return { kind: "anchor", fragment: raw };
  const hash = raw.indexOf("#");
  const fragment = hash < 0 ? "" : raw.slice(hash);
  let path = hash < 0 ? raw : raw.slice(0, hash);
  try {
    path = decodeURIComponent(path);
  } catch {
    return { kind: "missing", target };
  }
  const directory = source.slice(0, source.lastIndexOf("/") + 1);
  const explicit = /^\.{1,2}\//.test(path);
  const local = normalize(directory + path);
  const root = normalize(path);
  const match = (candidate: string | null) =>
    candidate === null
      ? undefined
      : files.find((file) => file.path === candidate);
  let found = match(path.startsWith("/") ? root : local);
  if (!found && !explicit) found = match(root);
  if (!found && local !== null && !/\.[a-z0-9]+$/i.test(path)) {
    const localMatches = files.filter(
      (file) => referenceStem(file.path) === local,
    );
    if (localMatches.length > 1) return { kind: "missing", target };
    found = localMatches[0];
  }
  if (!found && !explicit) {
    const stem = referenceStem(path);
    const matches = files.filter(
      (file) =>
        referenceStem(file.path) === stem ||
        referenceStem(file.path).endsWith(`/${stem}`),
    );
    if (matches.length === 1) found = matches[0];
  }
  return found
    ? { kind: "vault", path: found.path, fragment }
    : { kind: "missing", target };
}
