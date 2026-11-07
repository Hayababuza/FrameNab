import { describe, expect, it } from "vitest";
import { expandTemplate } from "./filename";

// Same cases as the Rust tests in framenab-export/src/still.rs.
describe("expandTemplate", () => {
  const now = new Date(2026, 2, 7, 9, 5, 2);
  it("matches the Rust implementation", () => {
    expect(expandTemplate("FrameNab_{yyyy-MM-dd_HH-mm-ss}", now)).toBe("FrameNab_2026-03-07_09-05-02");
    expect(expandTemplate("shot {dd.MM.yy} at {HH}h", now)).toBe("shot 07.03.26 at 09h");
    expect(expandTemplate("no braces", now)).toBe("no braces");
    expect(expandTemplate("bad/chars:{yyyy", now)).toBe("bad_chars_{yyyy");
    expect(expandTemplate("  ", now)).toBe("FrameNab");
  });
});
