import { describe, expect, it } from "vitest";
import type { SessionConfigOption } from "@/types";
import {
  applyAssignments,
  assignmentsFromOptions,
  hasAcpSessionMode,
  isRenderableConfigOption,
} from "./session-config";

const options: SessionConfigOption[] = [
  {
    id: "mode",
    name: "Mode",
    type: "select",
    current_value: "ask",
    options: [
      { value: "ask", name: "Ask" },
      { value: "code", name: "Code" },
    ],
  },
  {
    id: "brave_mode",
    name: "Brave mode",
    type: "boolean",
    current_value: false,
  },
  {
    id: "temperature",
    name: "Temperature",
    type: "number",
    current_value: 0.2,
  },
];

describe("ACP session config state", () => {
  it("builds assignments only for supported option types", () => {
    expect(assignmentsFromOptions(options)).toEqual([
      { config_id: "mode", value: { type: "id", value: "ask" } },
      {
        config_id: "brave_mode",
        value: { type: "boolean", value: false },
      },
    ]);
    expect(options.map(isRenderableConfigOption)).toEqual([true, true, false]);
  });

  it("applies pending values without reordering the agent snapshot", () => {
    const updated = applyAssignments(options, [
      { config_id: "mode", value: { type: "id", value: "code" } },
      {
        config_id: "brave_mode",
        value: { type: "boolean", value: true },
      },
    ]);
    expect(updated.map((option) => option.id)).toEqual([
      "mode",
      "brave_mode",
      "temperature",
    ]);
    expect(updated.map((option) => option.current_value)).toEqual([
      "code",
      true,
      0.2,
    ]);
  });

  it("detects when ACP provides the session mode control", () => {
    expect(hasAcpSessionMode(options)).toBe(true);
    expect(
      hasAcpSessionMode([{ ...options[0], id: "custom", category: "mode" }]),
    ).toBe(true);
    expect(hasAcpSessionMode(options.slice(1))).toBe(false);
  });
});
