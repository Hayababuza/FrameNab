import { describe, expect, it } from "vitest";
import type { Project } from "$lib/bindings/Project";
import type { Watermark } from "$lib/bindings/Watermark";
import { progressAnnotation, watermarkRect } from "./overlays";

// Same cases as the Rust tests in `framenapper_core::overlays`.
function project(): Project {
  return {
    version: 1,
    width: 200,
    height: 100,
    frames: [100, 100, 200].map((delayMs, i) => ({ id: `f${i}`, file: "", delayMs })),
    annotations: [],
    loop: 0,
    nextFrameId: 3,
    effects: { cursor: false, clicks: false, keys: false },
    overlays: { progress: null, watermark: null },
  };
}

describe("overlays", () => {
  it("fills the progress bar up to the end of each frame", () => {
    const p = project();
    expect(progressAnnotation(p, 0)).toBeNull();
    p.overlays.progress = { bottom: true, height: 4, color: { r: 255, g: 59, b: 48, a: 255 } };
    const rect = (i: number) => {
      const shape = progressAnnotation(p, i)!.shape;
      return shape.type === "rect" ? [shape.rect.w, shape.rect.y] : null;
    };
    expect(rect(0)).toEqual([50, 96]);
    expect(rect(2)).toEqual([200, 96]);
  });

  it("puts the watermark into the chosen corner", () => {
    const wm: Watermark = {
      file: "",
      imageWidth: 100,
      imageHeight: 50,
      anchor: "bottomRight",
      size: 0.25,
      opacity: 1,
    };
    expect(watermarkRect(200, 100, wm)).toEqual({ x: 148, y: 73, w: 50, h: 25 });
    const tall: Watermark = { ...wm, imageWidth: 10, imageHeight: 100, size: 1, anchor: "center" };
    expect(watermarkRect(200, 100, tall)).toEqual({ x: 95, y: 0, w: 10, h: 100 });
  });
});
