import { describe, expect, it } from "vitest";
import { History } from "./history";

describe("History", () => {
  it("undoes and redoes", () => {
    const h = new History<{ n: number }>();
    let state = { n: 1 };
    h.record(state);
    state = { n: 2 };
    state = h.undo(state)!;
    expect(state.n).toBe(1);
    state = h.redo(state)!;
    expect(state.n).toBe(2);
    expect(h.canRedo).toBe(false);
  });

  it("stores snapshots, not references", () => {
    const h = new History<{ n: number }>();
    const state = { n: 1 };
    h.record(state);
    state.n = 99;
    expect(h.undo(state)?.n).toBe(1);
  });

  it("respects the limit", () => {
    const h = new History<number>(2);
    [1, 2, 3].forEach((n) => h.record(n));
    expect(h.undo(0)).toBe(3);
    expect(h.undo(3)).toBe(2);
    expect(h.undo(2)).toBeUndefined();
  });
});
