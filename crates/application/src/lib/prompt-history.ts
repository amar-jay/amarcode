export type PromptSnapshot = { value: string; start: number; end: number };

/** Bounded draft history; typing is grouped, programmatic replacements are atomic. */
export class PromptHistory {
  past: PromptSnapshot[] = [];
  future: PromptSnapshot[] = [];
  current: PromptSnapshot;
  private lastEdit = 0;
  private lastKind = "";
  constructor(value = "") {
    this.current = { value, start: value.length, end: value.length };
  }
  selection(start: number, end: number) {
    if (start !== this.current.start || end !== this.current.end)
      this.lastKind = "";
    this.current = { ...this.current, start, end };
  }
  set(
    value: string,
    selection?: { start: number; end: number },
    now = Date.now(),
  ) {
    if (value === this.current.value) return;
    const delta = value.length - this.current.value.length;
    const kind =
      selection &&
      this.current.start === this.current.end &&
      Math.abs(delta) === 1
        ? delta > 0
          ? "insert"
          : "delete"
        : "";
    if (
      !kind ||
      kind !== this.lastKind ||
      now - this.lastEdit > 800 ||
      this.future.length
    ) {
      this.past.push({ ...this.current });
      if (this.past.length > 100) this.past.shift();
    }
    this.future = [];
    this.current = {
      value,
      start: selection?.start ?? value.length,
      end: selection?.end ?? value.length,
    };
    this.lastEdit = now;
    this.lastKind = kind;
  }
  undo() {
    return this.move(this.past, this.future);
  }
  redo() {
    return this.move(this.future, this.past);
  }
  private move(from: PromptSnapshot[], to: PromptSnapshot[]) {
    const next = from.pop();
    if (!next) return null;
    to.push({ ...this.current });
    this.current = next;
    this.lastKind = "";
    return next;
  }
}
