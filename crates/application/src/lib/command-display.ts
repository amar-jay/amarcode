/** Strip presentation wrappers agents sometimes add around executable text. */
export function cleanCommand(value: string): string {
  let trimmed = value.trim();
  const htmlCode = trimmed.match(/^<code(?:\s[^>]*)?>([\s\S]*)<\/code>$/i);
  if (htmlCode) trimmed = htmlCode[1].trim();

  const fencedCode = trimmed.match(/^```[^\n]*\n?([\s\S]*?)\n?```$/);
  if (fencedCode) trimmed = fencedCode[1].trim();

  const inlineCode = trimmed.match(/^`([\s\S]*)`$/);
  if (inlineCode) trimmed = inlineCode[1];

  if (trimmed.length >= 2 && trimmed.startsWith('"') && trimmed.endsWith('"')) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

function shellQuote(value: string): string {
  if (value === "") return "''";
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(value)) return value;
  return `'${value.replaceAll("'", `'"'"'`)}'`;
}

/** Render an executable and argv without losing argument boundaries. */
export function exactCommand(command: string, args: unknown): string {
  const executable = cleanCommand(command);
  if (!Array.isArray(args)) return executable;
  const argv = args.filter((arg): arg is string => typeof arg === "string");
  return [executable, ...argv.map(shellQuote)].join(" ");
}
