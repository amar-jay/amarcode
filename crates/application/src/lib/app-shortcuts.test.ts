import { expect, test, vi } from "vitest";
import {
  APP_SHORTCUTS,
  matchesShortcut,
  routeAppShortcut,
  type AppCommand,
} from "./app-shortcuts";

function key(key: string, options: Partial<KeyboardEvent> = {}) {
  return {
    key,
    ctrlKey: true,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    isComposing: false,
    repeat: false,
    defaultPrevented: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    ...options,
  };
}

test("routes Ctrl and Cmd shortcuts with exact modifiers", () => {
  const dispatch = vi.fn((_id: AppCommand) => true);
  routeAppShortcut(key("P"), () => false, dispatch);
  routeAppShortcut(
    key("F", { ctrlKey: false, metaKey: true, shiftKey: true }),
    () => false,
    dispatch,
  );
  expect(dispatch.mock.calls.map((call) => call[0])).toEqual([
    "find-files",
    "search-content",
  ]);
  expect(matchesShortcut(key("b", { shiftKey: true }), { key: "b" })).toBe(
    false,
  );
  expect(matchesShortcut(key("b", { ctrlKey: false }), { key: "b" })).toBe(
    false,
  );
  expect(matchesShortcut(key("b", { metaKey: true }), { key: "b" })).toBe(
    false,
  );
});

test("typing, IME, AltGr, repeats, and handled editor keys stay untouched", () => {
  for (const options of [
    { ctrlKey: false },
    { isComposing: true },
    { altKey: true },
    { repeat: true },
    { defaultPrevented: true },
  ]) {
    const event = key("s", options);
    const dispatch = vi.fn(() => true);
    routeAppShortcut(event, () => false, dispatch);
    expect(dispatch).not.toHaveBeenCalled();
    expect(event.preventDefault).not.toHaveBeenCalled();
  }
});

test("dialogs and menus block app actions without allowing browser reload", () => {
  const event = key("r", { shiftKey: true });
  const dispatch = vi.fn(() => true);
  routeAppShortcut(event, () => true, dispatch);
  expect(dispatch).not.toHaveBeenCalled();
  expect(event.preventDefault).toHaveBeenCalledOnce();
});

test("unavailable app actions cannot trigger browser print or save", () => {
  const event = key("p");
  routeAppShortcut(
    event,
    () => false,
    () => false,
  );
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(event.stopPropagation).not.toHaveBeenCalled();
});

test("chords are unique and contextual find stays with the editor", () => {
  for (const shortcut of APP_SHORTCUTS) {
    const definition: { key: string; mod?: boolean; shift?: boolean } =
      shortcut;
    const event = key(definition.key, {
      ctrlKey: definition.mod !== false,
      shiftKey: Boolean(definition.shift),
    });
    expect(
      APP_SHORTCUTS.filter((candidate) => matchesShortcut(event, candidate)),
    ).toEqual([shortcut]);
  }
  expect(
    APP_SHORTCUTS.some((shortcut) => matchesShortcut(key("f"), shortcut)),
  ).toBe(false);
});

test("Mod+K toggles its own palette but cannot bypass another modal", () => {
  const dispatch = vi.fn((_id: AppCommand) => true);
  routeAppShortcut(
    key("k"),
    () => true,
    dispatch,
    () => true,
  );
  expect(dispatch).toHaveBeenCalledWith("command-palette");
  dispatch.mockClear();
  routeAppShortcut(
    key("k"),
    () => true,
    dispatch,
    () => false,
  );
  expect(dispatch).not.toHaveBeenCalled();
});

test("the command palette uses Mod+K and no longer binds F1", () => {
  const palette = APP_SHORTCUTS.find((item) => item.id === "command-palette")!;
  expect(matchesShortcut(key("k"), palette)).toBe(true);
  expect(
    matchesShortcut(key("k", { ctrlKey: false, metaKey: true }), palette),
  ).toBe(true);
  expect(
    APP_SHORTCUTS.some((item) =>
      matchesShortcut(key("F1", { ctrlKey: false }), item),
    ),
  ).toBe(false);
});
