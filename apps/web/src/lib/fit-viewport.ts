/*
 * Viewport-fitting math (pure): popup/preview placement must never land
 * outside — or partially outside — the visible area. Shared by anything we
 * position by hand (CM hover previews); Base UI's Positioner already does
 * flip/shift for its own components.
 */

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Viewport {
  width: number;
  height: number;
}

/** Translate a box just inside the viewport edges (padding, never negative). */
export function fitIntoViewport(
  box: Box,
  viewport: Viewport,
  padding = 8,
): { x: number; y: number } {
  const maxX = Math.max(padding, viewport.width - padding - box.width);
  const maxY = Math.max(padding, viewport.height - padding - box.height);
  return {
    x: Math.min(Math.max(box.x, padding), maxX),
    y: Math.min(Math.max(box.y, padding), maxY),
  };
}

/**
 * Horizontal placement (right preferred, flip to left) with the vertical
 * follow-clamp: the popup trails its anchor's top edge but never leaves the
 * viewport. Used where a list (e.g. the completion tooltip) keeps its own
 * position and we hang a preview beside it.
 */
export function placeBesideHorizontally(
  anchor: Box,
  size: { width: number; height: number },
  viewport: Viewport,
  gap = 8,
  padding = 8,
): { x: number; y: number; side: "right" | "left" } {
  const rightX = anchor.x + anchor.width + gap;
  const leftX = anchor.x - size.width - gap;
  const rightFits = rightX + size.width <= viewport.width - padding;
  const leftFits = leftX >= padding;
  const side: "right" | "left" = rightFits || !leftFits ? "right" : "left";
  const x = Math.min(
    Math.max(side === "right" ? rightX : leftX, padding),
    Math.max(padding, viewport.width - padding - size.width),
  );
  const y = Math.min(
    Math.max(anchor.y, padding),
    Math.max(padding, viewport.height - padding - size.height),
  );
  return { x, y, side };
}
