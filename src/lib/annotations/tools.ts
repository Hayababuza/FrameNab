/** Annotation tools of the screenshot overlay: which shape a drag creates, defaults, shortcuts. */
import type { Annotation } from "$lib/bindings/Annotation";
import type { Point } from "$lib/bindings/Point";
import type { Rgba } from "$lib/bindings/Rgba";
import type { Shape } from "$lib/bindings/Shape";
import { rectFromPoints } from "./geometry";

export type Tool =
  | "select"
  | "arrow"
  | "line"
  | "rect"
  | "ellipse"
  | "pen"
  | "highlighter"
  | "text"
  | "counter"
  | "click"
  | "pixelate"
  | "blur";

/** SVG path data (24×24 viewBox) and keyboard shortcut per tool. */
export const tools: { id: Tool; key: string; icon: string }[] = [
  { id: "select", key: "v", icon: "M5 3l13 7-6 1.5L9.5 18z" },
  { id: "arrow", key: "a", icon: "M5 19L19 5M19 5h-8M19 5v8" },
  { id: "line", key: "l", icon: "M5 19L19 5" },
  { id: "rect", key: "r", icon: "M4 6h16v12H4z" },
  { id: "ellipse", key: "e", icon: "M4 12a8 6 0 1 0 16 0a8 6 0 1 0-16 0" },
  { id: "pen", key: "p", icon: "M3 21l3.5-1L19 7.5 16.5 5 4 17.5zM14.5 7l2.5 2.5" },
  { id: "highlighter", key: "h", icon: "M9 15l-4 4h6l1.5-1.5M9 15l8-8 3 3-8 8z" },
  { id: "text", key: "t", icon: "M5 7V5h14v2M12 5v14M9 19h6" },
  { id: "counter", key: "n", icon: "M4 12a8 8 0 1 0 16 0a8 8 0 1 0-16 0M10.5 9.5l2-1.5v8" },
  { id: "click", key: "k", icon: "M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0M8 12a4 4 0 1 0 8 0a4 4 0 1 0-8 0" },
  { id: "pixelate", key: "x", icon: "M4 4h5v5H4zM9 9h6v6H9zM15 4h5v5h-5zM4 15h5v5H4zM15 15h5v5h-5z" },
  { id: "blur", key: "b", icon: "M12 3s6 7 6 11a6 6 0 0 1-12 0c0-4 6-11 6-11z" },
];

export const toolForKey = (key: string): Tool | undefined => tools.find((t) => t.key === key)?.id;

export const colors: Rgba[] = [
  { r: 0xff, g: 0x3b, b: 0x30, a: 255 },
  { r: 0xff, g: 0x95, b: 0x00, a: 255 },
  { r: 0xff, g: 0xcc, b: 0x00, a: 255 },
  { r: 0x34, g: 0xc7, b: 0x59, a: 255 },
  { r: 0x0a, g: 0x84, b: 0xff, a: 255 },
  { r: 0xaf, g: 0x52, b: 0xde, a: 255 },
  { r: 0x1c, g: 0x1c, b: 0x1e, a: 255 },
  { r: 0xff, g: 0xff, b: 0xff, a: 255 },
];

/** Stroke widths offered in the toolbar, in screen (CSS) pixels. */
export const widths = [2, 4, 8];

export const fontSizeFor = (width: number) => 12 + width * 4;

export const toHex = (c: Rgba) => "#" + [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("");

export function fromHex(hex: string): Rgba {
  const n = parseInt(hex.replace("#", ""), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 255 };
}

/** Tools that create a shape by dragging (all except select, text and counter). */
export const isDragTool = (tool: Tool) => !["select", "text", "counter", "click"].includes(tool);

/** Shape created by dragging from `start` to `end` (`points` = the whole path, for pens). */
export function shapeFromDrag(
  tool: Tool,
  start: Point,
  end: Point,
  points: Point[],
  width: number,
): Shape | null {
  switch (tool) {
    case "arrow":
    case "line":
      return { type: tool, from: start, to: end };
    case "rect":
    case "ellipse":
      return { type: tool, rect: rectFromPoints(start, end) };
    case "pen":
    case "highlighter":
      return { type: tool, points };
    case "pixelate":
      return {
        type: "pixelate",
        rect: rectFromPoints(start, end),
        blockSize: Math.max(6, Math.round(width * 3)),
      };
    case "blur":
      return { type: "blur", rect: rectFromPoints(start, end), radius: Math.max(4, width * 2.5) };
    default:
      return null;
  }
}

/** Shapes too small to be intentional (a click instead of a drag). */
export function isTrivial(shape: Shape, minSize = 3): boolean {
  switch (shape.type) {
    case "arrow":
    case "line":
      return Math.hypot(shape.to.x - shape.from.x, shape.to.y - shape.from.y) < minSize;
    case "rect":
    case "ellipse":
    case "pixelate":
    case "blur":
      return shape.rect.w < minSize || shape.rect.h < minSize;
    case "pen":
    case "highlighter":
      return shape.points.length < 2;
    case "text":
      return shape.text.trim() === "";
    case "counter":
    case "click":
      return false;
  }
}

/** Next number for a counter marker. */
export function nextCounterValue(annotations: Annotation[]): number {
  return (
    annotations.reduce((max, a) => (a.shape.type === "counter" ? Math.max(max, a.shape.value) : max), 0) + 1
  );
}
