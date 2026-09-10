import type { JsonValue } from "@/types";

export const PERMISSION_MODES = [
  "confirm",
  "autoedit",
  "full-agentic",
] as const;
export type PermissionMode = (typeof PERMISSION_MODES)[number];

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function shouldAutoApprove(
  mode: PermissionMode,
  details: JsonValue,
): boolean {
  if (mode === "full-agentic") return true;
  if (mode !== "autoedit") return false;
  const record = asRecord(details);
  const tool = asRecord(record?.toolCall) ?? record;
  if (!tool) return false;
  const rawInput = asRecord(tool.rawInput);
  if (typeof rawInput?.command === "string") return false;
  return (
    tool.kind === "edit" ||
    tool.name === "write_file" ||
    tool.toolName === "write_file" ||
    typeof rawInput?.path === "string"
  );
}

export function automaticApprovalResult(details: JsonValue): JsonValue {
  const record = asRecord(details);
  const options = Array.isArray(record?.options) ? record.options : [];
  const parsed = options.flatMap((value) => {
    const option = asRecord(value);
    const optionId =
      typeof option?.optionId === "string"
        ? option.optionId
        : typeof option?.option_id === "string"
          ? option.option_id
          : null;
    return optionId
      ? [
          {
            optionId,
            kind: typeof option?.kind === "string" ? option.kind : "",
          },
        ]
      : [];
  });
  const selected =
    parsed.find((option) => option.kind === "allow_always") ??
    parsed.find((option) => option.kind === "allow_once") ??
    parsed.find((option) => /allow|approve|yes/i.test(option.optionId));
  return selected
    ? { outcome: { outcome: "selected", optionId: selected.optionId } }
    : { allow: true };
}
