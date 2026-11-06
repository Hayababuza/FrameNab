import { describe, expect, it } from "vitest";
import { beginDrag, toImagePixels, updateDrag } from "./selection";

const bounds = { w: 100, h: 100 };

describe("selection", () => {
  it("creates a selection in any drag direction", () => {
    const drag = beginDrag(null, { x: 50, y: 50 });
    expect(updateDrag(drag, { x: 10, y: 20 }, bounds)).toEqual({ x: 10, y: 20, w: 40, h: 30 });
  });

  it("clamps new selections to the screen", () => {
    const drag = beginDrag(null, { x: 50, y: 50 });
    expect(updateDrag(drag, { x: 150, y: -5 }, bounds)).toEqual({ x: 50, y: 0, w: 50, h: 50 });
  });

  it("moves an existing selection when dragging inside it", () => {
    const sel = { x: 10, y: 10, w: 20, h: 20 };
    const drag = beginDrag(sel, { x: 15, y: 15 });
    expect(drag.kind).toBe("move");
    expect(updateDrag(drag, { x: 25, y: 35 }, bounds)).toEqual({ x: 20, y: 30, w: 20, h: 20 });
    expect(updateDrag(drag, { x: 500, y: 500 }, bounds)).toEqual({ x: 80, y: 80, w: 20, h: 20 });
  });

  it("starts a new selection outside the current one", () => {
    expect(beginDrag({ x: 10, y: 10, w: 5, h: 5 }, { x: 50, y: 50 }).kind).toBe("create");
  });

  it("converts to image pixels for HiDPI", () => {
    expect(toImagePixels({ x: 10.2, y: 5, w: 20, h: 10 }, 2, { width: 3840, height: 2160 })).toEqual({
      x: 20,
      y: 10,
      width: 40,
      height: 20,
    });
    expect(toImagePixels({ x: 1900, y: 0, w: 50, h: 0 }, 1, { width: 1920, height: 1080 })).toEqual({
      x: 1900,
      y: 0,
      width: 20,
      height: 1,
    });
  });
});
