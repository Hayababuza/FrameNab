import { describe, expect, it } from "vitest";
import { beginDrag, handleAt, toImagePixels, toolbarPosition, updateDrag, windowAt } from "./selection";

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

  it("places the toolbar below, above or inside the selection", () => {
    const toolbar = { w: 200, h: 40 };
    const screen = { w: 1000, h: 800 };
    expect(toolbarPosition({ x: 100, y: 100, w: 400, h: 200 }, toolbar, screen)).toEqual({ x: 300, y: 308 });
    expect(toolbarPosition({ x: 100, y: 500, w: 400, h: 280 }, toolbar, screen)).toEqual({ x: 300, y: 452 });
    expect(toolbarPosition({ x: 0, y: 0, w: 1000, h: 800 }, toolbar, screen)).toEqual({ x: 792, y: 752 });
    expect(toolbarPosition({ x: 0, y: 100, w: 50, h: 50 }, toolbar, screen).x).toBe(8);
  });

  it("finds handles on the border", () => {
    const sel = { x: 100, y: 100, w: 200, h: 100 };
    expect(handleAt(sel, { x: 102, y: 98 })).toBe("nw");
    expect(handleAt(sel, { x: 200, y: 200 })).toBe("s");
    expect(handleAt(sel, { x: 305, y: 150 })).toBe("e");
    expect(handleAt(sel, { x: 200, y: 150 })).toBeNull();
  });

  it("resizes from a handle and flips past the opposite edge", () => {
    const sel = { x: 100, y: 100, w: 200, h: 100 };
    const bounds = { w: 1000, h: 1000 };
    const corner = beginDrag(sel, { x: 300, y: 200 });
    expect(corner.kind).toBe("resize");
    expect(updateDrag(corner, { x: 400, y: 250 }, bounds)).toEqual({ x: 100, y: 100, w: 300, h: 150 });
    expect(updateDrag(corner, { x: 50, y: 150 }, bounds)).toEqual({ x: 50, y: 100, w: 50, h: 50 });
    const edge = beginDrag(sel, { x: 200, y: 100 });
    expect(updateDrag(edge, { x: 999, y: 20 }, bounds)).toEqual({ x: 100, y: 20, w: 200, h: 180 });
  });

  it("finds the topmost window under the cursor and clamps it to the screen", () => {
    const windows = [
      { x: 100, y: 100, w: 200, h: 200 }, // on top
      { x: -50, y: 0, w: 2000, h: 1000 }, // behind, partly off screen
    ];
    const screen = { w: 960, h: 540 };
    expect(windowAt(windows, { x: 150, y: 150 }, 2, screen)).toEqual({ x: 50, y: 50, w: 100, h: 100 });
    expect(windowAt(windows, { x: 10, y: 10 }, 2, screen)).toEqual({ x: 0, y: 0, w: 960, h: 500 });
    expect(windowAt(windows, { x: 10, y: 1500 }, 2, screen)).toBeNull();
  });
});
