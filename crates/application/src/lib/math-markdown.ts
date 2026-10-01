function transformOutsideCodeSpans(
  line: string,
  transform: (text: string) => string,
): string {
  return line
    .split(/(`+[^`]*`+)/g)
    .map((segment, index) => (index % 2 === 0 ? transform(segment) : segment))
    .join("");
}

function looksLikeMathBlock(lines: string[]): boolean {
  const content = lines.join("\n").trim();
  return (
    /\\(?:boxed|frac|sqrt|sum|prod|int|lim|begin|end|qquad|pm|ne|neq|le|ge)\b/.test(
      content,
    ) || /[A-Za-z0-9})\]]\s*(?:=|<|>|\\(?:ne|neq|le|ge))\s*/.test(content)
  );
}

/**
 * Normalize the common math delimiters emitted by ACP agents to Streamdown's
 * dollar delimiters. Fenced/inline code is preserved verbatim, and plain
 * Markdown bracket blocks are converted only when their contents look like an
 * equation.
 */
export function normalizeMathMarkdown(markdown: string): string {
  const lines = markdown.split("\n");
  const output: string[] = [];
  let fence: "`" | "~" | null = null;

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const fenceMatch = /^\s*(`{3,}|~{3,})/.exec(line);
    if (fenceMatch) {
      const marker = fenceMatch[1][0] as "`" | "~";
      if (fence === null) fence = marker;
      else if (fence === marker) fence = null;
      output.push(line);
      continue;
    }
    if (fence !== null) {
      output.push(line);
      continue;
    }

    if (line.trim() === "[") {
      let closing = index + 1;
      while (closing < lines.length && lines[closing].trim() !== "]") closing++;
      const body = lines.slice(index + 1, closing);
      if (
        closing < lines.length &&
        body.length > 0 &&
        looksLikeMathBlock(body)
      ) {
        output.push("$$", ...body, "$$");
        index = closing;
        continue;
      }
    }

    output.push(
      transformOutsideCodeSpans(line, (text) =>
        text
          .replace(/\\\[/g, () => "$$")
          .replace(/\\\]/g, () => "$$")
          .replace(/\\\(/g, "$")
          .replace(/\\\)/g, "$"),
      ),
    );
  }

  return output.join("\n");
}
