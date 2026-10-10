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

/** Placement stays with the caller. Only dimensions and the drag lifecycle live here. */
export function attachPreviewResize(
  surface: HTMLElement,
  handle: HTMLButtonElement,
  anchor: () =>
    { left: number; right: number; top: number; bottom: number } | undefined,
  onResize?: () => void,
) {
  let preferred = preferredPreviewSize();
  let dragging: {
    x: number;
    y: number;
    width: number;
    height: number;
    corner: PreviewCorner;
    scale: number;
  } | null = null;
  const apply = (size: PreviewSize) => {
    surface.style.width = `${size.width}px`;
    surface.style.height = `${size.height}px`;
    onResize?.();
  };
  const updateCorner = () => {
    if (!dragging)
      handle.dataset.corner = previewCorner(
        surface.getBoundingClientRect(),
        anchor(),
      );
  };
  apply(preferred);
  const observer = new ResizeObserver(updateCorner);
  observer.observe(surface);
  window.addEventListener("resize", updateCorner);
  window.addEventListener("scroll", updateCorner, true);
  const down = (event: PointerEvent) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    updateCorner();
    const rect = surface.getBoundingClientRect();
    const scale = rect.width / surface.offsetWidth || 1;
    dragging = {
      x: event.clientX,
      y: event.clientY,
      width: rect.width / scale,
      height: rect.height / scale,
      corner: handle.dataset.corner as PreviewCorner,
      scale,
    };
    handle.setPointerCapture(event.pointerId);
    surface.dataset.resizing = "true";
  };
  const move = (event: PointerEvent) => {
    if (!dragging) return;
    const { x, y, width, height, corner, scale } = dragging;
    preferred = {
      width: Math.max(
        220,
        Math.min(
          (window.innerWidth - 24) / scale,
          width +
            ((event.clientX - x) / scale) * (corner.endsWith("w") ? -1 : 1),
        ),
      ),
      height: Math.max(
        160,
        Math.min(
          (window.innerHeight - 24) / scale,
          height +
            ((event.clientY - y) / scale) * (corner.startsWith("n") ? -1 : 1),
        ),
      ),
    };
    apply(preferred);
  };
  const finish = () => {
    if (!dragging) return;
    dragging = null;
    delete surface.dataset.resizing;
    try {
      localStorage.setItem(SIZE_KEY, JSON.stringify(preferred));
    } catch {
      /* Optional preference. */
    }
    updateCorner();
  };
  const keydown = (event: KeyboardEvent) => {
    if (
      !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
    )
      return;
    event.preventDefault();
    event.stopPropagation();
    const step = event.shiftKey ? 40 : 16;
    updateCorner();
    const rect = surface.getBoundingClientRect();
    const scale = rect.width / surface.offsetWidth || 1;
    const corner = handle.dataset.corner!;
    preferred = {
      width: Math.max(
        220,
        Math.min(
          (window.innerWidth - 24) / scale,
          rect.width / scale +
            (event.key === "ArrowRight"
              ? step
              : event.key === "ArrowLeft"
                ? -step
                : 0) *
              (corner.endsWith("w") ? -1 : 1),
        ),
      ),
      height: Math.max(
        160,
        Math.min(
          (window.innerHeight - 24) / scale,
          rect.height / scale +
            (event.key === "ArrowDown"
              ? step
              : event.key === "ArrowUp"
                ? -step
                : 0) *
              (corner.startsWith("n") ? -1 : 1),
        ),
      ),
    };
    apply(preferred);
    try {
      localStorage.setItem(SIZE_KEY, JSON.stringify(preferred));
    } catch {
      /* Optional preference. */
    }
  };
  handle.addEventListener("pointerdown", down);
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", finish);
  handle.addEventListener("lostpointercapture", finish);
  handle.addEventListener("keydown", keydown);
  updateCorner();
  return {
    update: updateCorner,
    destroy() {
      observer.disconnect();
      window.removeEventListener("resize", updateCorner);
      window.removeEventListener("scroll", updateCorner, true);
      handle.removeEventListener("pointerdown", down);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", finish);
      handle.removeEventListener("lostpointercapture", finish);
      handle.removeEventListener("keydown", keydown);
    },
  };
}
