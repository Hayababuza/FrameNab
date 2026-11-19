/**
 * Helpers of the inspection tools (measure and color picker) shared by the screenshot overlay
 * and the editor: zooming the view, measuring in whole image pixels, reading pixel colors.
 */
import type { Point } from "$lib/bindings/Point";
import type { Rect } from "$lib/bindings/Rect";
import type { Rgba } from "$lib/bindings/Rgba";
import type { Tool } from "./tools";

/** The tools that only look at the image (and can zoom) instead of drawing on it. */
export const isInspectTool = (tool: Tool) => tool === "measure" || tool === "picker";

/**
 * Zoom on top of the normal (fitted) view: a point at `p` in the normal view is shown at
 * `p * z + (x, y)`. `z = 1` and no offset is the normal view.
 */
export interface Zoom {
  z: number;
  x: number;
  y: number;
}

export const NO_ZOOM: Zoom = { z: 1, x: 0, y: 0 };
export const MAX_ZOOM = 32;

/** Where a point of the normal view is shown. */
export const zoomed = (v: Zoom, p: Point): Point => ({ x: p.x * v.z + v.x, y: p.y * v.z + v.y });

/** The point of the normal view shown at `p`. */
export const unzoomed = (v: Zoom, p: Point): Point => ({ x: (p.x - v.x) / v.z, y: (p.y - v.y) / v.z });

/** Keeps the zoomed content covering the whole `area` (no empty margins when panning). */
function clampOffset(v: Zoom, area: { w: number; h: number }): Zoom {
  const clamp = (value: number, size: number) => Math.min(0, Math.max(size - size * v.z, value));
  return { z: v.z, x: clamp(v.x, area.w), y: clamp(v.y, area.h) };
}

/** Zooms by `factor`, keeping the point under `at` (in view coordinates) where it is. */
export function zoomAt(v: Zoom, factor: number, at: Point, area: { w: number; h: number }): Zoom {
  const z = Math.min(MAX_ZOOM, Math.max(1, v.z * factor));
  if (z === 1) return NO_ZOOM;
  const k = z / v.z;
  return clampOffset({ z, x: at.x - (at.x - v.x) * k, y: at.y - (at.y - v.y) * k }, area);
}

/** Moves the zoomed view by `dx`, `dy` (view coordinates). */
export const panBy = (v: Zoom, dx: number, dy: number, area: { w: number; h: number }): Zoom =>
  clampOffset({ z: v.z, x: v.x + dx, y: v.y + dy }, area);

/** The measured rect between two points (image pixels): on pixel edges, inside the image. */
export function measureRect(a: Point, b: Point, width: number, height: number): Rect {
  const cx = (v: number) => Math.round(Math.max(0, Math.min(width, v)));
  const cy = (v: number) => Math.round(Math.max(0, Math.min(height, v)));
  const x0 = cx(Math.min(a.x, b.x));
  const y0 = cy(Math.min(a.y, b.y));
  return { x: x0, y: y0, w: cx(Math.max(a.x, b.x)) - x0, h: cy(Math.max(a.y, b.y)) - y0 };
}

/** Color of the pixel at `p` (image pixels) in RGBA `pixels`, or null outside the image. */
export function pixelAt(pixels: Uint8ClampedArray, width: number, height: number, p: Point): Rgba | null {
  const x = Math.floor(p.x);
  const y = Math.floor(p.y);
  if (x < 0 || y < 0 || x >= width || y >= height) return null;
  const i = (y * width + x) * 4;
  return { r: pixels[i], g: pixels[i + 1], b: pixels[i + 2], a: pixels[i + 3] };
}

export const hexOf = (c: Rgba) =>
  "#" +
  [c.r, c.g, c.b]
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("")
    .toUpperCase();

export const rgbOf = (c: Rgba) => `rgb(${c.r}, ${c.g}, ${c.b})`;
