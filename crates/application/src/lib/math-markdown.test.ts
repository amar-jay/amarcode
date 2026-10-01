import { describe, expect, it } from "vitest";
import { normalizeMathMarkdown } from "./math-markdown";

describe("normalizeMathMarkdown", () => {
  it("normalizes standard LaTeX parenthesis and bracket delimiters", () => {
    expect(normalizeMathMarkdown(String.raw`Inline \(x^2\) here.`)).toBe(
      "Inline $x^2$ here.",
    );
    expect(normalizeMathMarkdown("\\[\nx^2 + y^2 = z^2\n\\]")).toBe(
      "$$\nx^2 + y^2 = z^2\n$$",
    );
  });

  it("normalizes equation-like bare bracket blocks", () => {
    expect(
      normalizeMathMarkdown(
        "Formula:\n\n[\n\\boxed{x=\\frac{-b\\pm\\sqrt{b^2-4ac}}{2a}}\n]",
      ),
    ).toBe("Formula:\n\n$$\n\\boxed{x=\\frac{-b\\pm\\sqrt{b^2-4ac}}{2a}}\n$$");
  });

  it("preserves ordinary brackets, inline code, and fenced code", () => {
    const markdown = [
      "[",
      "ordinary prose",
      "]",
      "Use `\\(not math\\)` literally.",
      "```text",
      "\\[not math\\]",
      "```",
    ].join("\n");
    expect(normalizeMathMarkdown(markdown)).toBe(markdown);
  });

  it("leaves dollar-delimited math unchanged", () => {
    const markdown = "$D>0$ and $$x = \\frac{-b}{2a}$$";
    expect(normalizeMathMarkdown(markdown)).toBe(markdown);
  });
});
