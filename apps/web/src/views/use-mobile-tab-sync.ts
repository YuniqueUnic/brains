import { useEffect, useRef, useState } from "react";
import type { EditorState } from "../components/editor/editor-core";
import type { MobileTab } from "./mobile-layout";
import { useIsMobile } from "../hooks/use-mobile";

/*
 * Mobile tab auto-switching (the mobile half of the pane contract): opening
 * a file, opening the properties panel or hitting a version conflict jumps
 * to its tab; the trash stays where the user is browsing. Entering mobile
 * keeps the files tab, and closing the panel never yanks the user back —
 * only the transitions watched below switch.
 */
export function useMobileTabSync(
  editor: EditorState,
  mode: "workspace" | "trash",
  propertiesOpen: boolean,
): [MobileTab, (tab: MobileTab) => void, boolean] {
  const isMobile = useIsMobile();
  const wasMobile = useRef(false);
  const wasPropertiesOpen = useRef(false);
  const wasEditorPath = useRef<string | null>(null);
  const wasConflict = useRef(false);
  const [tab, setTab] = useState<MobileTab>("files");
  const activeEditorPath =
    editor.kind === "empty" || editor.kind === "loading" ? null : editor.path;
  useEffect(() => {
    if (!isMobile) {
      wasMobile.current = false;
      return;
    }
    const entering = !wasMobile.current;
    wasMobile.current = true;
    if (entering || mode !== "workspace") return;
    const conflict = editor.kind === "conflict";
    if (conflict && !wasConflict.current) setTab("editor");
    else if (propertiesOpen && !wasPropertiesOpen.current) setTab("info");
    else if (activeEditorPath !== null && activeEditorPath !== wasEditorPath.current)
      setTab("editor");
    wasPropertiesOpen.current = propertiesOpen;
    wasEditorPath.current = activeEditorPath;
    wasConflict.current = conflict;
  }, [isMobile, mode, propertiesOpen, activeEditorPath, editor.kind]);
  return [tab, setTab, isMobile];
}
