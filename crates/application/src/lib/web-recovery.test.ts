import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";
const script = readFileSync(
  new URL("../../public/web-recovery.js", import.meta.url),
  "utf8",
);
function setup() {
  const listeners: Record<string, (event: unknown) => void> = {};
  const root = { hidden: false };
  const elements: { textContent?: string; click?: () => void }[] = [];
  const append = vi.fn();
  const reload = vi.fn();
  runInNewContext(script, {
    window: {
      addEventListener: (name: string, callback: (event: unknown) => void) => {
        listeners[name] = callback;
      },
      location: { reload },
    },
    document: {
      getElementById: () => root,
      body: { append },
      createElement: () => {
        const element = {
          textContent: "",
          style: { cssText: "" },
          setAttribute: vi.fn(),
          append: vi.fn(),
          addEventListener: (_name: string, callback: () => void) => {
            (element as { click?: () => void }).click = callback;
          },
        };
        elements.push(element);
        return element;
      },
    },
  });
  return { listeners, root, elements, append, reload };
}
describe("web module failure recovery", () => {
  it("keeps a recoverable screen outside React's root after an old chunk fails", () => {
    const view = setup();
    view.listeners.unhandledrejection({
      reason: new Error("error loading dynamically imported module"),
    });
    expect(view.root.hidden).toBe(true);
    expect(view.append).toHaveBeenCalledTimes(1);
    expect(
      view.elements.some(
        (element) => element.textContent === "Reload application",
      ),
    ).toBe(true);
    view.elements.at(-1)?.click?.();
    expect(view.reload).toHaveBeenCalledOnce();
    view.listeners.error({
      message: "Failed to fetch dynamically imported module",
    });
    expect(view.append).toHaveBeenCalledTimes(1);
  });
  it("catches entry module failures but leaves ordinary API failures alone", () => {
    const view = setup();
    view.listeners.unhandledrejection({ reason: new Error("HTTP 502") });
    expect(view.append).not.toHaveBeenCalled();
    view.listeners.error({ target: { tagName: "SCRIPT", type: "module" } });
    expect(view.append).toHaveBeenCalledOnce();
  });
});
