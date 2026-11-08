/**
 * Input effects (cursor highlight, click ripples, key captions) as annotations for one moment of
 * a recording. Mirrors `framenab_core::input::effect_annotations` (Rust), which renders the
 * result; keep both in sync (same tests).
 */
import type { Annotation } from "$lib/bindings/Annotation";
import type { Effects } from "$lib/bindings/Effects";
import type { InputEvents } from "$lib/bindings/InputEvents";
import type { Point } from "$lib/bindings/Point";

export const CLICK_MS = 500;
export const KEY_MS = 1500;

/** Mouse position at time `t`, interpolated between samples. */
export function cursorAt(input: InputEvents, t: number): Point | null {
  const samples = input.cursor;
  let after = samples.findIndex((s) => s.t > t);
  if (after === -1) after = samples.length;
  const a = samples[after - 1];
  const b = samples[after];
  if (a && b && b.t > a.t) {
    const f = (t - a.t) / (b.t - a.t);
    return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
  }
  const s = a ?? b;
  return s ? { x: s.x, y: s.y } : null;
}

const style = (r: number, g: number, b: number, width: number, opacity: number) => ({
  stroke: { r, g, b, a: 255 },
  width,
  fill: null,
  opacity,
});

export function effectAnnotations(
  input: InputEvents,
  effects: Effects,
  t: number,
  width: number,
  height: number,
): Annotation[] {
  const unit = Math.max(Math.min(width, height) / 40, 8);
  const out: Annotation[] = [];
  const base = { frameStart: 0, frameEnd: 4294967295 };
  const p = effects.cursor ? cursorAt(input, t) : null;
  if (p) {
    const r = unit * 1.6;
    out.push({
      ...base,
      id: "fx-cursor",
      shape: { type: "ellipse", rect: { x: p.x - r, y: p.y - r, w: 2 * r, h: 2 * r } },
      style: { ...style(255, 214, 0, 1, 0.35), fill: { r: 255, g: 214, b: 0, a: 255 } },
    });
  }
  if (effects.clicks) {
    input.clicks.forEach((c, i) => {
      if (c.t <= t && t < c.t + CLICK_MS) {
        const color = c.button === 1 ? [10, 132, 255] : c.button === 2 ? [52, 199, 89] : [255, 59, 48];
        out.push({
          ...base,
          id: `fx-click-${i}`,
          shape: { type: "click", at: { x: c.x, y: c.y } },
          style: style(color[0], color[1], color[2], unit * 0.5, 1),
        });
      }
    });
  }
  if (effects.keys) {
    const recent = input.keys.filter((k) => k.t <= t && t < k.t + KEY_MS).map((k) => k.label);
    if (recent.length) {
      const fontSize = unit * 1.4;
      const text = recent.slice(-4).join("  ");
      const approxWidth = [...text].length * fontSize * 0.6;
      out.push({
        ...base,
        id: "fx-keys",
        shape: {
          type: "text",
          at: { x: (width - approxWidth) / 2, y: height - fontSize * 2.6 },
          text,
          fontSize,
          background: { r: 20, g: 20, b: 24, a: 220 },
        },
        style: style(255, 255, 255, 1, 1),
      });
    }
  }
  return out;
}
