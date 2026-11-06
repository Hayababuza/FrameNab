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

/** Is `p` on or inside the shape? `tolerance` enlarges the hit area; `width` is the stroke width. */
export function hitTest(shape: Shape, p: Point, tolerance = 4, width = 4): boolean {
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
      return inRect(p, shapeBounds(shape), tolerance);
    case "counter":
      return Math.hypot(p.x - shape.at.x, p.y - shape.at.y) <= counterRadius(width) + tolerance;
  }
}

/** Sizes derived from the stroke width; mirror `framenab_render::{counter_radius, text_padding}`. */
export const LINE_HEIGHT = 1.25;
export const counterRadius = (width: number) => 10 + width * 2;
export const textPadding = (fontSize: number) => Math.round(fontSize * 0.2);

/** Moves a shape by `(dx, dy)`; mirrors `Shape::translated` in Rust. */
export function translateShape(shape: Shape, dx: number, dy: number): Shape {
  const p = (q: Point): Point => ({ x: q.x + dx, y: q.y + dy });
  const r = (q: Rect): Rect => ({ ...q, x: q.x + dx, y: q.y + dy });
  switch (shape.type) {
    case "arrow":
    case "line":
      return { ...shape, from: p(shape.from), to: p(shape.to) };
    case "rect":
    case "ellipse":
    case "blur":
    case "pixelate":
      return { ...shape, rect: r(shape.rect) };
    case "pen":
    case "highlighter":
      return { ...shape, points: shape.points.map(p) };
    case "text":
    case "counter":
      return { ...shape, at: p(shape.at) };
  }
}

/** Approximate bounding box of a shape (text width is estimated). */
export function shapeBounds(shape: Shape, width = 0): Rect {
  const fromPoints = (pts: Point[]): Rect => {
    const xs = pts.map((q) => q.x);
    const ys = pts.map((q) => q.y);
    const x = Math.min(...xs);
    const y = Math.min(...ys);
    return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
  };
  switch (shape.type) {
    case "arrow":
    case "line":
      return fromPoints([shape.from, shape.to]);
    case "pen":
    case "highlighter":
      return fromPoints(shape.points);
    case "rect":
    case "ellipse":
    case "blur":
    case "pixelate":
      return normalizeRect(shape.rect);
    case "text": {
      const lines = shape.text.split("\n");
      const longest = Math.max(...lines.map((l) => l.length));
      return {
        x: shape.at.x,
        y: shape.at.y,
        w: longest * shape.fontSize * 0.6,
        h: lines.length * shape.fontSize * LINE_HEIGHT,
      };
    }
    case "counter": {
      const r = counterRadius(width);
      return { x: shape.at.x - r, y: shape.at.y - r, w: 2 * r, h: 2 * r };
    }
  }
}
