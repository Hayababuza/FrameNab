/** Geometry helpers for the annotation tools (dragging out shapes, hit-testing for selection). */
import type { Point } from "$lib/bindings/Point";
import type { Rect } from "$lib/bindings/Rect";
import type { Shape } from "$lib/bindings/Shape";

/** Rect with positive width/height; mirrors `Rect::normalized` in Rust. */
export function normalizeRect(r: Rect): Rect {
  return {
    x: r.w < 0 ? r.x + r.w : r.x,
    y: r.h < 0 ? r.y + r.h : r.y,
    w: Math.abs(r.w),
    h: Math.abs(r.h),
  };
}

export function rectFromPoints(a: Point, b: Point): Rect {
  return normalizeRect({ x: a.x, y: a.y, w: b.x - a.x, h: b.y - a.y });
}

export function distanceToSegment(p: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const lenSq = dx * dx + dy * dy;
  const t = lenSq === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / lenSq));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

function inRect(p: Point, r: Rect, tolerance: number): boolean {
  const n = normalizeRect(r);
  return (
    p.x >= n.x - tolerance &&
    p.x <= n.x + n.w + tolerance &&
    p.y >= n.y - tolerance &&
    p.y <= n.y + n.h + tolerance
  );
}

/** Is `p` on or inside the shape? `tolerance` enlarges the hit area (e.g. stroke width / 2). */
export function hitTest(shape: Shape, p: Point, tolerance = 4): boolean {
  switch (shape.type) {
    case "arrow":
    case "line":
      return distanceToSegment(p, shape.from, shape.to) <= tolerance;
    case "pen":
    case "highlighter":
      return shape.points.some((q, i) =>
        i === 0
          ? Math.hypot(p.x - q.x, p.y - q.y) <= tolerance
          : distanceToSegment(p, shape.points[i - 1], q) <= tolerance,
      );
    case "rect":
    case "ellipse":
    case "blur":
    case "pixelate":
      return inRect(p, shape.rect, tolerance);
    case "text":
      return inRect(
        p,
        {
          x: shape.at.x,
          y: shape.at.y,
          w: shape.text.length * shape.fontSize * 0.6,
          h: shape.fontSize * 1.3,
        },
        tolerance,
      );
    case "counter":
      return Math.hypot(p.x - shape.at.x, p.y - shape.at.y) <= 16 + tolerance;
  }
}
