import type {
  SessionConfigAssignment,
  SessionConfigOption,
  SessionConfigValue,
} from "@/types";

const storageKey = (agentId: string) => `amarcode-session-config:${agentId}`;

export function loadLastSessionConfig(agentId: string): SessionConfigOption[] {
  if (!agentId) return [];
  try {
    const raw = localStorage.getItem(storageKey(agentId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as SessionConfigOption[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function rememberSessionConfig(
  agentId: string,
  options: SessionConfigOption[],
) {
  if (!agentId) return;
  localStorage.setItem(storageKey(agentId), JSON.stringify(options));
}

export function assignmentsFromOptions(
  options: SessionConfigOption[],
): SessionConfigAssignment[] {
  return options.flatMap((option) => {
    const value = assignmentFromOption(option);
    return value ? [{ config_id: option.id, value }] : [];
  });
}

export function assignmentFromOption(
  option: SessionConfigOption,
): SessionConfigValue | null {
  if (option.type === "select" && typeof option.current_value === "string") {
    return { type: "id", value: option.current_value };
  }
  if (option.type === "boolean" && typeof option.current_value === "boolean") {
    return { type: "boolean", value: option.current_value };
  }
  return null;
}

export function applyAssignments(
  options: SessionConfigOption[],
  assignments: SessionConfigAssignment[],
): SessionConfigOption[] {
  if (!assignments.length) return options;
  const byId = new Map(assignments.map((item) => [item.config_id, item.value]));
  return options.map((option) => {
    const next = byId.get(option.id);
    if (!next) return option;
    if (next.type === "id") {
      return { ...option, current_value: next.value };
    }
    if (next.type === "boolean") {
      return { ...option, current_value: next.value };
    }
    return option;
  });
}

export function isRenderableConfigOption(option: SessionConfigOption): boolean {
  return option.type === "select" || option.type === "boolean";
}

/** Whether the agent owns session-mode / approval behavior through ACP. */
export function hasAcpSessionMode(options: SessionConfigOption[]): boolean {
  return options.some(
    (option) => option.category === "mode" || option.id === "mode",
  );
}
