export interface PreviewSize {
  width: number;
  height: number;
}
export type PreviewCorner = "nw" | "ne" | "sw" | "se";
const SIZE_KEY = "brains.preview.size";
const DEFAULT_SIZE: PreviewSize = { width: 380, height: 320 };

export function preferredPreviewSize(): PreviewSize {
  try {
    const size = JSON.parse(
      localStorage.getItem(SIZE_KEY) ?? "null",
    ) as PreviewSize | null;
    if (
      size &&
      Number.isFinite(size.width) &&
      Number.isFinite(size.height) &&
      size.width >= 220 &&
      size.height >= 160
    )
      return size;
  } catch {
    /* Storage is optional, preview remains usable. */
  }
  return DEFAULT_SIZE;
}

export function previewCorner(
  card: DOMRect,
  anchor?: { left: number; right: number; top: number; bottom: number },
): PreviewCorner {
  const edge = 24;
  const x =
    card.right >= window.innerWidth - edge
      ? window.innerWidth
      : card.left <= edge
        ? 0
        : anchor
          ? (anchor.left + anchor.right) / 2
          : window.innerWidth / 2;
  const y = anchor ? (anchor.top + anchor.bottom) / 2 : window.innerHeight / 2;
  return `${(card.top + card.bottom) / 2 < y ? "n" : "s"}${(card.left + card.right) / 2 < x ? "w" : "e"}`;
}

/** Native hosts own automatic placement; this frame owns manual geometry. */
export interface PreviewFrame {
  element: HTMLElement;
  // Completion rewrites its info element's entire inline style, so its stable
  // parent owns the variables. Other hosts use the frame itself.
  styleOwner: HTMLElement;
}

export function attachPreviewResize(
  surface: HTMLElement,
  handle: HTMLButtonElement,
  anchor: () =>
    { left: number; right: number; top: number; bottom: number } | undefined,
  frame: () => PreviewFrame,
  onResize?: () => void,
) {
  let preferred = preferredPreviewSize();
  let manual: PreviewFrame | null = null;
  let dragging: {
    x: number;
    y: number;
    rect: DOMRect;
    corner: PreviewCorner;
    scale: number;
  } | null = null;
  const applySize = () => {
    surface.style.width = `${preferred.width}px`;
    surface.style.height = `${preferred.height}px`;
  };
  const save = () => {
    try {
      localStorage.setItem(SIZE_KEY, JSON.stringify(preferred));
    } catch {
      /* Optional preference. */
    }
  };
  const updateCorner = () => {
    if (!manual)
      handle.dataset.corner = previewCorner(
        surface.getBoundingClientRect(),
        anchor(),
      );
  };
  const finish = () => {
    if (!dragging) return;
    dragging = null;
    delete surface.dataset.resizing;
    save();
  };
  let cornerFrame = 0;
  const reset = () => {
    finish();
    cancelAnimationFrame(cornerFrame);
    cornerFrame = requestAnimationFrame(updateCorner);
    if (!manual) return;
    delete manual.element.dataset.previewPlacement;
    manual.styleOwner.style.removeProperty("--preview-left");
    manual.styleOwner.style.removeProperty("--preview-top");
    manual = null;
    onResize?.();
  };
  const resize = (
    rect: DOMRect,
    corner: PreviewCorner,
    scale: number,
    dx: number,
    dy: number,
  ) => {
    const west = corner.endsWith("w"),
      north = corner.startsWith("n");
    const maxWidth = Math.max(
      1,
      west ? rect.right - 12 : innerWidth - 12 - rect.left,
    );
    const maxHeight = Math.max(
      1,
      north ? rect.bottom - 12 : innerHeight - 12 - rect.top,
    );
    const width = Math.min(
      maxWidth,
      Math.max(
        Math.min(220 * scale, maxWidth),
        rect.width + dx * (west ? -1 : 1),
      ),
    );
    const height = Math.min(
      maxHeight,
      Math.max(
        Math.min(160 * scale, maxHeight),
        rect.height + dy * (north ? -1 : 1),
      ),
    );
    manual ??= frame();
    manual.styleOwner.style.setProperty(
      "--preview-left",
      `${west ? rect.right - width : rect.left}px`,
    );
    manual.styleOwner.style.setProperty(
      "--preview-top",
      `${north ? rect.bottom - height : rect.top}px`,
    );
    manual.element.dataset.previewPlacement = "manual";
    preferred = { width: width / scale, height: height / scale };
    applySize();
  };
  const down = (event: PointerEvent) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const rect = surface.getBoundingClientRect();
    dragging = {
      x: event.clientX,
      y: event.clientY,
      rect,
      corner: handle.dataset.corner as PreviewCorner,
      scale: rect.width / surface.offsetWidth || 1,
    };
    handle.setPointerCapture(event.pointerId);
    surface.dataset.resizing = "true";
  };
  const move = (event: PointerEvent) => {
    if (!dragging) return;
    const { rect, corner, scale, x, y } = dragging;
    resize(rect, corner, scale, event.clientX - x, event.clientY - y);
  };
  const keydown = (event: KeyboardEvent) => {
    if (
      !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
    )
      return;
    event.preventDefault();
    event.stopPropagation();
    updateCorner();
    const rect = surface.getBoundingClientRect(),
      scale = rect.width / surface.offsetWidth || 1;
    const step = (event.shiftKey ? 40 : 16) * scale;
    resize(
      rect,
      handle.dataset.corner as PreviewCorner,
      scale,
      event.key === "ArrowRight" ? step : event.key === "ArrowLeft" ? -step : 0,
      event.key === "ArrowDown" ? step : event.key === "ArrowUp" ? -step : 0,
    );
    save();
  };
  const scroll = (event: Event) => {
    if (event.target instanceof Node && surface.contains(event.target)) return;
    reset();
  };
  applySize();
  onResize?.();
  const observer = new ResizeObserver(updateCorner);
  observer.observe(surface);
  window.addEventListener("resize", reset);
  window.addEventListener("scroll", scroll, true);
  surface.addEventListener("preview-reset-placement", reset);
  handle.addEventListener("pointerdown", down);
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", finish);
  handle.addEventListener("pointercancel", finish);
  handle.addEventListener("lostpointercapture", finish);
  handle.addEventListener("keydown", keydown);
  updateCorner();
  return {
    destroy() {
      reset();
      cancelAnimationFrame(cornerFrame);
      observer.disconnect();
      window.removeEventListener("resize", reset);
      window.removeEventListener("scroll", scroll, true);
      surface.removeEventListener("preview-reset-placement", reset);
      handle.removeEventListener("pointerdown", down);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", finish);
      handle.removeEventListener("pointercancel", finish);
      handle.removeEventListener("lostpointercapture", finish);
      handle.removeEventListener("keydown", keydown);
    },
  };
}
