/** Session interaction mode for a prompt / chat composer. */
export const SESSION_MODES = ["plan", "build", "ask"] as const;
export type SessionMode = (typeof SESSION_MODES)[number];

export function isSessionMode(value: unknown): value is SessionMode {
  return value === "plan" || value === "build" || value === "ask";
}

export function parseSessionMode(
  value: string | null | undefined,
  fallback: SessionMode = "build",
): SessionMode {
  return isSessionMode(value) ? value : fallback;
}
