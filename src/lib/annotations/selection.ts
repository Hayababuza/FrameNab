/** Area selection in the screenshot overlay (pure logic, unit-tested). */
import type { Point } from "$lib/bindings/Point";
import type { Rect } from "$lib/bindings/Rect";
import { rectFromPoints } from "./geometry";

/** Resize handles: n/s/e/w edges and the four corners. */
export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

export const handles: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

export type Drag =
  | { kind: "create"; origin: Point }
  | { kind: "move"; offset: Point; start: Rect }
  | { kind: "resize"; handle: Handle; start: Rect };

export const handleCursor: Record<Handle, string> = {
  nw: "nwse-resize",
  se: "nwse-resize",
  ne: "nesw-resize",
  sw: "nesw-resize",
  n: "ns-resize",
  s: "ns-resize",
  e: "ew-resize",
  w: "ew-resize",
};

export function contains(r: Rect, p: Point): boolean {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

/** Center of a handle on the selection border. */
export function handlePoint(r: Rect, h: Handle): Point {
  const x = h.includes("w") ? r.x : h.includes("e") ? r.x + r.w : r.x + r.w / 2;
  const y = h.startsWith("n") ? r.y : h.startsWith("s") ? r.y + r.h : r.y + r.h / 2;
  return { x, y };
}

/**
 * Handle under `p`, if any. `radius` is the grab distance in CSS pixels; it shrinks for small
 * selections so their inside can still be grabbed to move them.
 */
export function handleAt(r: Rect, p: Point, radius = 8): Handle | null {
  const reach = Math.min(radius, Math.min(r.w, r.h) / 4);
  const hit = handles.find((h) => {
    const c = handlePoint(r, h);
    return Math.abs(p.x - c.x) < reach && Math.abs(p.y - c.y) < reach;
  });
  return hit ?? null;
}

/** Starts a drag: on a handle it resizes, inside the selection it moves it, elsewhere it creates a new one. */
export function beginDrag(selection: Rect | null, p: Point): Drag {
  if (selection) {
    const handle = handleAt(selection, p);
    if (handle) return { kind: "resize", handle, start: selection };
    if (contains(selection, p)) {
      return { kind: "move", offset: { x: p.x - selection.x, y: p.y - selection.y }, start: selection };
    }
  }
  return { kind: "create", origin: p };
}

/** Selection while dragging to `p`, kept inside `bounds` (width × height). */
export function updateDrag(drag: Drag, p: Point, bounds: { w: number; h: number }): Rect {
  const clamp = (v: number, max: number) => Math.min(Math.max(v, 0), max);
  const q = { x: clamp(p.x, bounds.w), y: clamp(p.y, bounds.h) };
  switch (drag.kind) {
    case "create":
      return rectFromPoints(drag.origin, q);
    case "resize": {
      // Move the grabbed edges, keep the opposite ones; dragging past the opposite edge flips.
      const { handle, start } = drag;
      const x0 = handle.includes("w") ? q.x : start.x;
      const x1 = handle.includes("e") ? q.x : start.x + start.w;
      const y0 = handle.startsWith("n") ? q.y : start.y;
      const y1 = handle.startsWith("s") ? q.y : start.y + start.h;
      return rectFromPoints({ x: x0, y: y0 }, { x: x1, y: y1 });
    }
    case "move": {
      const { w, h } = drag.start;
      return {
        x: clamp(p.x - drag.offset.x, bounds.w - w),
        y: clamp(p.y - drag.offset.y, bounds.h - h),
        w,
        h,
      };
    }
  }
}

/** Converts a selection in CSS pixels to whole image pixels. */
export function toImagePixels(r: Rect, scale: number, image: { width: number; height: number }) {
  const x = Math.max(0, Math.round(r.x * scale));
  const y = Math.max(0, Math.round(r.y * scale));
  return {
    x,
    y,
    width: Math.max(1, Math.min(image.width - x, Math.round(r.w * scale))),
    height: Math.max(1, Math.min(image.height - y, Math.round(r.h * scale))),
  };
}

/**
 * Where to place the toolbar for a selection: below it (right-aligned), otherwise above it,
 * otherwise inside its bottom edge. Always kept on screen.
 */
export function toolbarPosition(
  selection: Rect,
  toolbar: { w: number; h: number },
  screen: { w: number; h: number },
  gap = 8,
): Point {
  const x = Math.min(Math.max(selection.x + selection.w - toolbar.w, gap), screen.w - toolbar.w - gap);
  const below = selection.y + selection.h + gap;
  const above = selection.y - gap - toolbar.h;
  let y: number;
  if (below + toolbar.h <= screen.h - gap) y = below;
  else if (above >= gap) y = above;
  else y = selection.y + selection.h - toolbar.h - gap;
  return { x, y: Math.max(gap, y) };
}
