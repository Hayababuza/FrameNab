import { describe, expect, it } from "vitest";
import { distanceToSegment, hitTest, normalizeRect, rectFromPoints } from "./geometry";

describe("geometry", () => {
  it("normalizes rects like the Rust model", () => {
    expect(normalizeRect({ x: 10, y: 10, w: -4, h: -6 })).toEqual({ x: 6, y: 4, w: 4, h: 6 });
    expect(rectFromPoints({ x: 5, y: 5 }, { x: 1, y: 9 })).toEqual({ x: 1, y: 5, w: 4, h: 4 });
  });

  it("measures distance to a segment", () => {
    expect(distanceToSegment({ x: 5, y: 3 }, { x: 0, y: 0 }, { x: 10, y: 0 })).toBe(3);
    expect(distanceToSegment({ x: 13, y: 4 }, { x: 0, y: 0 }, { x: 10, y: 0 })).toBe(5);
  });

  it("hit-tests shapes", () => {
    const arrow = { type: "arrow", from: { x: 0, y: 0 }, to: { x: 100, y: 0 } } as const;
    expect(hitTest(arrow, { x: 50, y: 3 })).toBe(true);
    expect(hitTest(arrow, { x: 50, y: 30 })).toBe(false);
    const rect = { type: "rect", rect: { x: 0, y: 0, w: 10, h: 10 } } as const;
    expect(hitTest(rect, { x: 12, y: 5 }, 4)).toBe(true);
    expect(hitTest(rect, { x: 20, y: 5 }, 4)).toBe(false);
  });
});
