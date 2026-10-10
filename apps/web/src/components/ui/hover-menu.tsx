import { useRef, useState, type ReactElement, type ReactNode } from "react";
import { Menu } from "@base-ui-components/react/menu";
import { menuPopupClass } from "./menu";

/*
 * Hover-open, click-to-pin menu (inkmark pattern, DESIGN.md §5-6): entering
 * the trigger opens the popup after a short delay so sweeping the header
 * doesn't cascade; leaving closes it — unless it was pinned by a click.
 * A click on the hover-opened trigger means "pin" (stay open on mouse-out),
 * a second click closes. Keyboard users toggle through Menu.Trigger as
 * usual: Enter opens, outside click or Esc closes.
 */
export function HoverMenu({
  trigger,
  children,
  align = "start",
  sideOffset = 6,
}: {
  /** The trigger element (a Button or similar); hover/click handlers are merged in. */
  trigger: ReactElement<Record<string, unknown>>;
  children: ReactNode;
  align?: "start" | "end";
  sideOffset?: number;
}) {
  const [open, setOpen] = useState(false);
  const openTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);
  const hoverTrigger = useRef(false);
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const popupRef = useRef<HTMLDivElement | null>(null);
  // Pin state is read inside event handlers only — a ref, not re-render state.
  const pinnedRef = useRef(false);

  const clearTimers = () => {
    if (openTimer.current !== null) window.clearTimeout(openTimer.current);
    if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    openTimer.current = null;
    closeTimer.current = null;
  };

  const setPinnedState = (value: boolean) => {
    pinnedRef.current = value;
  };

  const scheduleOpen = () => {
    clearTimers();
    if (open) return;
    openTimer.current = window.setTimeout(() => setOpen(true), 120);
  };

  const scheduleClose = () => {
    clearTimers();
    if (!open || pinnedRef.current) return;
    // 300ms grace, and the close is pointer-verified: portals can make the
    // browser fire a synthetic mouseleave at a stationary cursor — only a
    // pointer that has truly left the trigger closes the menu. (Keyboard
    // opens pin the menu, so focus staying in the popup never fights this.)
    closeTimer.current = window.setTimeout(() => {
      if (!wrapperRef.current?.matches(":hover") && !popupRef.current?.matches(":hover")) {
        setOpen(false);
      }
    }, 300);
  };

  const handleOpenChange = (next: boolean) => {
    clearTimers();
    if (next) {
      // Opened by click/Enter (hover-open goes through the timer): pin it,
      // per the inkmark interaction — a clicked menu stays until dismissed.
      setPinnedState(true);
      setOpen(true);
      return;
    }
    if (hoverTrigger.current && !pinnedRef.current) {
      // Closing request while the pointer is on the trigger = the pin click
      // (Base UI's toggle would otherwise close what hover just opened).
      setPinnedState(true);
      setOpen(true);
      return;
    }
    setOpen(false);
    setPinnedState(false);
  };

  return (
    // modal={false}: a modal menu mounts a viewport-wide inert layer that
    // covers the trigger — synthetic mouseleave at a stationary cursor, zoom
    // or not. Hover menus must be non-modal.
    <Menu.Root open={open} onOpenChange={handleOpenChange} modal={false}>
      <div
        ref={wrapperRef}
        className="flex items-center"
        onMouseEnter={() => {
          hoverTrigger.current = true;
          scheduleOpen();
        }}
        onMouseLeave={() => {
          hoverTrigger.current = false;
          scheduleClose();
        }}
      >
        <Menu.Trigger render={trigger} />
      </div>
      <Menu.Portal>
        <Menu.Positioner align={align} sideOffset={sideOffset}>
          <Menu.Popup
            ref={popupRef}
            className={menuPopupClass}
            onMouseEnter={clearTimers}
            onMouseLeave={scheduleClose}
          >
            {children}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
