import { useEffect, useRef, useState } from "react";

type Entry = { session: string; index: number };
type Destination = { path: string | null; entry: Entry };
type Pending =
  | { kind: "push"; path: string }
  | ({ kind: "pop" | "restore" | "follow" } & Destination)
  | { kind: "traverse" };

const currentPath = () => new URL(location.href).searchParams.get("file");
const limitKey = (entry: Entry) => `brains.navigation.${entry.session}`;

/** Browser history is the file history; only its forward bound is stored per tab. */
export function useFileNavigation({
  open,
  isOpen,
  isDirty,
  onBlocked,
}: {
  open: (path: string | null) => void;
  isOpen: (path: string) => boolean;
  isDirty: () => boolean;
  onBlocked: (path: string | null) => void;
}) {
  const callbacks = useRef({ open, isOpen, isDirty, onBlocked });
  callbacks.current = { open, isOpen, isDirty, onBlocked };
  const [entry, setEntry] = useState<Entry>(() => {
    const saved = history.state?.brainsNavigation as Entry | undefined;
    return saved ?? { session: crypto.randomUUID(), index: 0 };
  });
  const current = useRef(entry);
  const pending = useRef<Pending | null>(null);
  const [lastIndex, setLastIndex] = useState(() =>
    Number(sessionStorage.getItem(limitKey(entry)) ?? entry.index),
  );
  const needsConfirmation = (path: string | null) =>
    callbacks.current.isDirty() && (path === null || !callbacks.current.isOpen(path));

  const accept = (next: Entry, path: string | null) => {
    current.current = next;
    setEntry(next);
    pending.current = null;
    callbacks.current.open(path);
  };
  const push = (path: string) => {
    // A URL survives a failed load or deletion. Retrying that document
    // reopens the existing entry instead of adding duplicate history.
    if (path === currentPath()) {
      accept(current.current, path);
      return;
    }
    const next = { ...current.current, index: current.current.index + 1 };
    const url = new URL(location.href);
    url.searchParams.set("file", path);
    url.hash = "";
    history.pushState({ ...history.state, brainsNavigation: next }, "", url);
    sessionStorage.setItem(limitKey(next), String(next.index));
    setLastIndex(next.index);
    accept(next, path);
  };

  useEffect(() => {
    history.replaceState(
      { ...history.state, brainsNavigation: current.current },
      "",
    );
    callbacks.current.open(currentPath());
    const pop = (event: PopStateEvent) => {
      const next = event.state?.brainsNavigation as Entry | undefined;
      if (!next || next.session !== current.current.session) return;
      const transition = pending.current;
      if (transition?.kind === "restore") {
        if (next.index !== current.current.index) {
          history.go(current.current.index - next.index);
        } else {
          pending.current = { ...transition, kind: "pop" };
          callbacks.current.onBlocked(transition.path);
        }
        return;
      }
      if (
        transition?.kind === "follow" &&
        next.index === transition.entry.index
      ) {
        accept(next, transition.path);
      } else if (next.index === current.current.index) {
        pending.current = null;
      } else if (needsConfirmation(currentPath())) {
        // popstate arrives after the URL moved. Restore it before prompting so
        // cancelling or refreshing keeps the document the user is still editing.
        pending.current = { kind: "restore", entry: next, path: currentPath() };
        history.go(current.current.index - next.index);
      } else accept(next, currentPath());
    };
    window.addEventListener("popstate", pop);
    return () => window.removeEventListener("popstate", pop);
  }, []);

  return {
    canBack: entry.index > 0,
    canForward: entry.index < lastIndex,
    back() {
      if (pending.current || current.current.index === 0) return;
      pending.current = { kind: "traverse" };
      history.back();
    },
    forward() {
      if (pending.current || current.current.index >= lastIndex) return;
      pending.current = { kind: "traverse" };
      history.forward();
    },
    navigate(path: string) {
      if (pending.current) return;
      if (needsConfirmation(path)) {
        pending.current = { kind: "push", path };
        callbacks.current.onBlocked(path);
      } else push(path);
    },
    confirm() {
      const next = pending.current;
      if (next?.kind === "push") push(next.path);
      else if (next?.kind === "pop") {
        pending.current = { ...next, kind: "follow" };
        history.go(next.entry.index - current.current.index);
      }
    },
    cancel() {
      pending.current = null;
    },
  };
}
