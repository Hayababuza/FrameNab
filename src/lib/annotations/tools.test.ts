import { describe, expect, it } from "vitest";
import type { Annotation } from "$lib/bindings/Annotation";
import type { Shape } from "$lib/bindings/Shape";
import { hitTest, shapeBounds, translateShape } from "./geometry";
import { fromHex, isTrivial, nextCounterValue, shapeFromDrag, toHex, toolForKey } from "./tools";

const a = { x: 10, y: 10 };
const b = { x: 40, y: 30 };

describe("tools", () => {
  it("creates shapes from drags", () => {
    expect(shapeFromDrag("arrow", a, b, [], 4)).toEqual({ type: "arrow", from: a, to: b });
    expect(shapeFromDrag("rect", b, a, [], 4)).toEqual({
      type: "rect",
      rect: { x: 10, y: 10, w: 30, h: 20 },
    });
    expect(shapeFromDrag("pixelate", a, b, [], 4)).toMatchObject({ type: "pixelate", blockSize: 12 });
    expect(shapeFromDrag("select", a, b, [], 4)).toBeNull();
  });

  it("detects accidental clicks", () => {
    expect(isTrivial(shapeFromDrag("line", a, { x: 11, y: 11 }, [], 4)!)).toBe(true);
    expect(isTrivial(shapeFromDrag("line", a, b, [], 4)!)).toBe(false);
    expect(isTrivial({ type: "text", at: a, text: "  ", fontSize: 20, background: null })).toBe(true);
  });

  it("numbers counters consecutively", () => {
    const counter = (value: number) =>
      ({ shape: { type: "counter", at: a, value } }) as unknown as Annotation;
    expect(nextCounterValue([])).toBe(1);
    expect(nextCounterValue([counter(1), counter(4)])).toBe(5);
  });

  it("maps shortcuts and colors", () => {
    expect(toolForKey("a")).toBe("arrow");
    expect(toolForKey("q")).toBeUndefined();
    expect(toHex({ r: 255, g: 59, b: 48, a: 255 })).toBe("#ff3b30");
    expect(fromHex("#0a84ff")).toEqual({ r: 10, g: 132, b: 255, a: 255 });
  });
});

describe("shape geometry", () => {
  it("translates every shape kind like Rust", () => {
    const pen: Shape = { type: "pen", points: [a, b] };
    expect(translateShape(pen, 1, 2)).toEqual({
      type: "pen",
      points: [
        { x: 11, y: 12 },
        { x: 41, y: 32 },
      ],
    });
    const text: Shape = { type: "text", at: a, text: "x", fontSize: 10, background: null };
    expect(translateShape(text, -10, 0)).toMatchObject({ at: { x: 0, y: 10 } });
  });

  it("computes bounds and hit-tests counters by radius", () => {
    const counter: Shape = { type: "counter", at: { x: 50, y: 50 }, value: 1 };
    expect(shapeBounds(counter, 4)).toEqual({ x: 32, y: 32, w: 36, h: 36 });
    expect(hitTest(counter, { x: 65, y: 50 }, 0, 4)).toBe(true);
    expect(hitTest(counter, { x: 70, y: 50 }, 0, 4)).toBe(false);
  });
});
