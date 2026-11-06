/**
 * Live preview of annotations on a canvas. Mirrors `framenab-render` (Rust), which produces the
 * actual result, so keep the two in sync. The context's transform must map image pixels to
 * device pixels (scale only, no rotation).
 */
import type { Annotation } from "$lib/bindings/Annotation";
import type { Point } from "$lib/bindings/Point";
import type { Rect } from "$lib/bindings/Rect";
import type { Rgba } from "$lib/bindings/Rgba";
import { counterRadius, LINE_HEIGHT, normalizeRect, shapeBounds, textPadding } from "./geometry";

const css = (c: Rgba, opacity = 1) => `rgba(${c.r}, ${c.g}, ${c.b}, ${(c.a / 255) * opacity})`;
export const FONT_FAMILY = "sans-serif";

function polyline(ctx: CanvasRenderingContext2D, points: Point[]) {
  if (points.length === 0) return;
  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);
  if (points.length === 1) ctx.lineTo(points[0].x + 0.01, points[0].y);
  for (const p of points.slice(1)) ctx.lineTo(p.x, p.y);
  ctx.stroke();
}

function arrowHead(ctx: CanvasRenderingContext2D, from: Point, to: Point, width: number) {
  const len = Math.hypot(to.x - from.x, to.y - from.y);
  if (len < 1e-3) return;
  const ux = (to.x - from.x) / len;
  const uy = (to.y - from.y) / len;
  const size = Math.max(width * 4, 10);
  const bx = to.x - ux * size;
  const by = to.y - uy * size;
  const px = -uy * size * 0.5;
  const py = ux * size * 0.5;
  ctx.beginPath();
  ctx.moveTo(to.x, to.y);
  ctx.lineTo(bx + px, by + py);
  ctx.lineTo(bx - px, by - py);
  ctx.closePath();
  ctx.fill();
}

/** Area in device pixels for an image-space rect, clamped to the canvas. */
function deviceRect(ctx: CanvasRenderingContext2D, r: Rect) {
  const m = ctx.getTransform();
  const n = normalizeRect(r);
  const x = Math.max(0, Math.floor(n.x * m.a + m.e));
  const y = Math.max(0, Math.floor(n.y * m.d + m.f));
  const x1 = Math.min(ctx.canvas.width, Math.ceil((n.x + n.w) * m.a + m.e));
  const y1 = Math.min(ctx.canvas.height, Math.ceil((n.y + n.h) * m.d + m.f));
  return x1 > x && y1 > y ? { x, y, w: x1 - x, h: y1 - y, scale: m.a } : null;
}

/** Re-draws an area of the canvas through a smaller copy: blocky for pixelate, soft for blur. */
function resample(ctx: CanvasRenderingContext2D, r: Rect, factor: number, smooth: boolean, passes = 1) {
  const area = deviceRect(ctx, r);
  if (!area) return;
  const tw = Math.max(1, Math.round(area.w / (factor * area.scale)));
  const th = Math.max(1, Math.round(area.h / (factor * area.scale)));
  const tmp = document.createElement("canvas");
  tmp.width = tw;
  tmp.height = th;
  const t = tmp.getContext("2d")!;
  t.imageSmoothingEnabled = true;
  t.drawImage(ctx.canvas, area.x, area.y, area.w, area.h, 0, 0, tw, th);
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.imageSmoothingEnabled = smooth;
  for (let i = 0; i < passes; i++) {
    ctx.drawImage(tmp, 0, 0, tw, th, area.x, area.y, area.w, area.h);
    if (i + 1 < passes) t.drawImage(ctx.canvas, area.x, area.y, area.w, area.h, 0, 0, tw, th);
  }
  ctx.restore();
}

export function textLines(text: string) {
  return text.split("\n");
}

export function drawAnnotation(ctx: CanvasRenderingContext2D, a: Annotation) {
  const { shape, style } = a;
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.lineWidth = Math.max(0.5, style.width);
  ctx.strokeStyle = css(style.stroke, style.opacity);
  ctx.fillStyle = css(style.stroke, style.opacity);

  switch (shape.type) {
    case "line":
      polyline(ctx, [shape.from, shape.to]);
      break;
    case "arrow":
      polyline(ctx, [shape.from, shape.to]);
      arrowHead(ctx, shape.from, shape.to, style.width);
      break;
    case "rect": {
      const r = normalizeRect(shape.rect);
      if (style.fill) {
        ctx.fillStyle = css(style.fill, style.opacity);
        ctx.fillRect(r.x, r.y, r.w, r.h);
      }
      ctx.strokeRect(r.x, r.y, r.w, r.h);
      break;
    }
    case "ellipse": {
      const r = normalizeRect(shape.rect);
      ctx.beginPath();
      ctx.ellipse(r.x + r.w / 2, r.y + r.h / 2, r.w / 2, r.h / 2, 0, 0, Math.PI * 2);
      if (style.fill) {
        ctx.fillStyle = css(style.fill, style.opacity);
        ctx.fill();
      }
      ctx.stroke();
      break;
    }
    case "pen":
      polyline(ctx, shape.points);
      break;
    case "highlighter":
      ctx.lineWidth = style.width * 3;
      ctx.strokeStyle = css(style.stroke, style.opacity * 0.4);
      polyline(ctx, shape.points);
      break;
    case "text": {
      ctx.font = `${shape.fontSize}px ${FONT_FAMILY}`;
      ctx.textBaseline = "middle";
      const lineHeight = shape.fontSize * LINE_HEIGHT;
      const lines = textLines(shape.text);
      if (shape.background) {
        const pad = textPadding(shape.fontSize);
        const w = Math.max(...lines.map((l) => ctx.measureText(l).width));
        ctx.fillStyle = css(shape.background, style.opacity);
        ctx.fillRect(shape.at.x - pad, shape.at.y - pad, w + 2 * pad, lines.length * lineHeight + 2 * pad);
        ctx.fillStyle = css(style.stroke, style.opacity);
      }
      lines.forEach((line, i) => ctx.fillText(line, shape.at.x, shape.at.y + lineHeight * (i + 0.5)));
      break;
    }
    case "counter": {
      const r = counterRadius(style.width);
      ctx.beginPath();
      ctx.arc(shape.at.x, shape.at.y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = `rgba(255, 255, 255, ${style.opacity})`;
      ctx.font = `bold ${r * 1.1}px ${FONT_FAMILY}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(shape.value), shape.at.x, shape.at.y);
      break;
    }
    case "pixelate":
      resample(ctx, shape.rect, Math.max(2, shape.blockSize), false);
      break;
    case "blur":
      resample(ctx, shape.rect, Math.max(2, shape.radius / 1.5), true, 2);
      break;
  }
  ctx.restore();
}

/** Dashed frame around the selected annotation. */
export function drawSelectionFrame(ctx: CanvasRenderingContext2D, a: Annotation, pixel: number) {
  const b = shapeBounds(a.shape, a.style.width);
  const pad = a.style.width / 2 + 4 * pixel;
  ctx.save();
  ctx.lineWidth = pixel;
  ctx.setLineDash([4 * pixel, 3 * pixel]);
  ctx.strokeStyle = "#ffffff";
  ctx.strokeRect(b.x - pad, b.y - pad, b.w + 2 * pad, b.h + 2 * pad);
  ctx.lineDashOffset = 4 * pixel;
  ctx.strokeStyle = "#000000";
  ctx.strokeRect(b.x - pad, b.y - pad, b.w + 2 * pad, b.h + 2 * pad);
  ctx.restore();
}
