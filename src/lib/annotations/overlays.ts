/**
 * Overlays over the whole recording (progress bar, watermark) for the editor preview. Mirrors
 * `framenab_core::overlays` (Rust), which renders the export; keep both in sync (same tests).
 */
import type { Annotation } from "$lib/bindings/Annotation";
import type { Project } from "$lib/bindings/Project";
import type { Rect } from "$lib/bindings/Rect";
import type { Watermark } from "$lib/bindings/Watermark";

/** The progress bar on frame `index` (full at the end of the last frame), if switched on. */
export function progressAnnotation(project: Project, index: number): Annotation | null {
  const bar = project.overlays.progress;
  if (!bar) return null;
  const total = Math.max(
    1,
    project.frames.reduce((sum, f) => sum + f.delayMs, 0),
  );
  const end = project.frames.slice(0, index + 1).reduce((sum, f) => sum + f.delayMs, 0);
  const h = Math.min(Math.max(bar.height, 1), project.height);
  const y = bar.bottom ? project.height - h : 0;
  return {
    id: "overlay-progress",
    frameStart: 0,
    frameEnd: 4294967295,
    shape: { type: "rect", rect: { x: 0, y, w: project.width * Math.min(end / total, 1), h } },
    style: { stroke: bar.color, width: 0, fill: bar.color, opacity: 1 },
  };
}

/** Where the watermark goes on a `width` × `height` frame (whole pixels). */
export function watermarkRect(width: number, height: number, wm: Watermark): Rect {
  const aspect = Math.max(wm.imageHeight, 1) / Math.max(wm.imageWidth, 1);
  let w = Math.max(Math.round(width * Math.min(Math.max(wm.size, 0.01), 1)), 1);
  let h = Math.max(Math.round(w * aspect), 1);
  if (h > height) {
    h = height;
    w = Math.max(Math.round(h / aspect), 1);
  }
  const margin = Math.round(Math.min(width, height) * 0.02);
  const [x, y] = {
    topLeft: [margin, margin],
    topRight: [width - w - margin, margin],
    bottomLeft: [margin, height - h - margin],
    bottomRight: [width - w - margin, height - h - margin],
    center: [Math.round((width - w) / 2), Math.round((height - h) / 2)],
  }[wm.anchor];
  return { x: Math.max(x, 0), y: Math.max(y, 0), w, h };
}
