import type { AvailableCommand } from "@/types";

/** Only complete the leading command token, never slashes in prose or paths. */
export function slashCommandQuery(text: string, cursor: number): string | null {
  if (!text.startsWith("/") || cursor < 1) return null;
  const tokenEnd = text.search(/\s/);
  if (tokenEnd !== -1 || cursor !== text.length) return null;
  return text.slice(1).toLocaleLowerCase();
}

export function filterSlashCommands(
  commands: AvailableCommand[],
  query: string,
) {
  return commands.filter(
    (command) =>
      command.name.toLocaleLowerCase().includes(query) ||
      command.description.toLocaleLowerCase().includes(query),
  );
}

export function commandInputHint(text: string, commands: AvailableCommand[]) {
  const match = /^\/([^\s]+)\s*$/.exec(text);
  return match
    ? commands.find((command) => command.name === match[1])?.input?.hint
    : undefined;
}
