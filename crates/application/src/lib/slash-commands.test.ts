import { describe, expect, it } from "vitest";
import {
  commandInputHint,
  filterSlashCommands,
  slashCommandQuery,
} from "./slash-commands";

const commands = [
  { name: "plan", description: "Plan a task", input: { hint: "task to plan" } },
  { name: "test", description: "Run project checks", input: null },
];

describe("slash command completion", () => {
  it("opens only on a leading command token at the cursor", () => {
    expect(slashCommandQuery("/", 1)).toBe("");
    expect(slashCommandQuery("/PL", 3)).toBe("pl");
    for (const text of [
      "explain /plan",
      "/plan args",
      "/plan\n",
      "/plan ",
      "",
    ]) {
      expect(slashCommandQuery(text, text.length)).toBeNull();
    }
    expect(slashCommandQuery("/plan", 2)).toBeNull();
  });

  it("filters names and descriptions and uses the latest input hints", () => {
    expect(
      filterSlashCommands(commands, "checks").map((command) => command.name),
    ).toEqual(["test"]);
    expect(commandInputHint("/plan ", commands)).toBe("task to plan");
    expect(commandInputHint("/plan my task", commands)).toBeUndefined();
    expect(commandInputHint("/plan ", [])).toBeUndefined();
  });
});
