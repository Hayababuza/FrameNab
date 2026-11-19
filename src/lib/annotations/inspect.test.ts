import { describe, expect, it } from "vitest";
import { hexOf, measureRect, NO_ZOOM, panBy, pixelAt, rgbOf, unzoomed, zoomAt, zoomed } from "./inspect";

const area = { w: 800, h: 600 };

describe("zoom", () => {
  it("keeps the point under the cursor in place", () => {
    const at = { x: 200, y: 150 };
    const v = zoomAt(NO_ZOOM, 4, at, area);
    expect(v.z).toBe(4);
    expect(zoomed(v, unzoomed(v, at))).toEqual(at);
    expect(unzoomed(v, at)).toEqual(at);
  });

  it("stays between 1× and the maximum and never shows empty margins", () => {
    expect(zoomAt(NO_ZOOM, 0.5, { x: 10, y: 10 }, area)).toEqual(NO_ZOOM);
    expect(zoomAt(NO_ZOOM, 1000, { x: 10, y: 10 }, area).z).toBe(32);
    const v = zoomAt(NO_ZOOM, 2, { x: 400, y: 300 }, area);
    expect(panBy(v, 10_000, 10_000, area)).toMatchObject({ x: 0, y: 0 });
    expect(panBy(v, -10_000, -10_000, area)).toMatchObject({ x: -800, y: -600 });
  });
});

describe("measure", () => {
  it("measures between pixel edges, in either direction, inside the image", () => {
    expect(measureRect({ x: 10.4, y: 20.6 }, { x: 110.5, y: 70 }, 1000, 1000)).toEqual({
      x: 10,
      y: 21,
      w: 101,
      h: 49,
    });
    expect(measureRect({ x: 50, y: 50 }, { x: -20, y: 5000 }, 100, 80)).toEqual({
      x: 0,
      y: 50,
      w: 50,
      h: 30,
    });
  });
});

describe("colors", () => {
  const pixels = new Uint8ClampedArray([255, 0, 0, 255, 10, 132, 255, 255]);

  it("reads the pixel under a point", () => {
    expect(pixelAt(pixels, 2, 1, { x: 1.7, y: 0.2 })).toEqual({ r: 10, g: 132, b: 255, a: 255 });
    expect(pixelAt(pixels, 2, 1, { x: 2, y: 0 })).toBeNull();
    expect(pixelAt(pixels, 2, 1, { x: -0.5, y: 0 })).toBeNull();
  });

  it("formats HEX and RGB", () => {
    const c = { r: 10, g: 132, b: 255, a: 255 };
    expect(hexOf(c)).toBe("#0A84FF");
    expect(rgbOf(c)).toBe("rgb(10, 132, 255)");
  });
});
