import { describe, expect, it } from "vitest";
import type { InputEvents } from "$lib/bindings/InputEvents";
import { cursorAt, effectAnnotations } from "./effects";

// Same cases as the Rust tests in framenab-core/src/input.rs.
const input: InputEvents = {
  cursor: [
    { t: 0, x: 0, y: 0 },
    { t: 100, x: 100, y: 50 },
  ],
  clicks: [{ t: 200, x: 10, y: 20, button: 0 }],
  keys: [{ t: 300, label: "Ctrl + C" }],
};

describe("effects", () => {
  it("interpolates and clamps the cursor", () => {
    expect(cursorAt(input, 50)).toEqual({ x: 50, y: 25 });
    expect(cursorAt(input, 500)).toEqual({ x: 100, y: 50 });
    expect(cursorAt({ cursor: [], clicks: [], keys: [] }, 5)).toBeNull();
  });

  it("shows effects only when enabled and in time", () => {
    const all = { cursor: true, clicks: true, keys: true };
    const kinds = (t: number) => effectAnnotations(input, all, t, 800, 600).map((a) => a.shape.type);
    expect(kinds(150)).toEqual(["ellipse"]);
    expect(kinds(250)).toEqual(["ellipse", "click"]);
    expect(kinds(350)).toEqual(["ellipse", "click", "text"]);
    expect(kinds(900)).toEqual(["ellipse", "text"]);
    expect(effectAnnotations(input, { cursor: false, clicks: false, keys: false }, 350, 800, 600)).toEqual(
      [],
    );
  });
});
