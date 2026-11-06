/**
 * Undo/redo via snapshots, mirroring `framenab_core::History`.
 * Used for annotations in the editor and the screenshot overlay.
 */
export class History<T> {
  #undo: T[] = [];
  #redo: T[] = [];

  constructor(private readonly limit = 100) {}

  /** Records the state *before* a change. */
  record(before: T): void {
    this.#undo.push(structuredClone(before));
    if (this.#undo.length > this.limit) this.#undo.shift();
    this.#redo = [];
  }

  undo(current: T): T | undefined {
    const prev = this.#undo.pop();
    if (prev !== undefined) this.#redo.push(structuredClone(current));
    return prev;
  }

  redo(current: T): T | undefined {
    const next = this.#redo.pop();
    if (next !== undefined) this.#undo.push(structuredClone(current));
    return next;
  }

  get canUndo(): boolean {
    return this.#undo.length > 0;
  }

  get canRedo(): boolean {
    return this.#redo.length > 0;
  }
}
