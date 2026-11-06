/** Area selection in the screenshot overlay (pure logic, unit-tested). */
import type { Point } from "$lib/bindings/Point";
import type { Rect } from "$lib/bindings/Rect";
import { rectFromPoints } from "./geometry";

export type Drag = { kind: "create"; origin: Point } | { kind: "move"; offset: Point; start: Rect };

export function contains(r: Rect, p: Point): boolean {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

/** Starts a drag: inside an existing selection it moves it, elsewhere it creates a new one. */
export function beginDrag(selection: Rect | null, p: Point): Drag {
  if (selection && contains(selection, p)) {
    return { kind: "move", offset: { x: p.x - selection.x, y: p.y - selection.y }, start: selection };
  }
  return { kind: "create", origin: p };
}

/** Selection while dragging to `p`, kept inside `bounds` (width × height). */
export function updateDrag(drag: Drag, p: Point, bounds: { w: number; h: number }): Rect {
  const clamp = (v: number, max: number) => Math.min(Math.max(v, 0), max);
  if (drag.kind === "create") {
    return rectFromPoints(drag.origin, { x: clamp(p.x, bounds.w), y: clamp(p.y, bounds.h) });
  }
  const { w, h } = drag.start;
  return {
    x: clamp(p.x - drag.offset.x, bounds.w - w),
    y: clamp(p.y - drag.offset.y, bounds.h - h),
    w,
    h,
  };
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
