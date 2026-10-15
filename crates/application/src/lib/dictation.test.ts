import { describe, expect, it } from "vitest";
import { formatDictationTime, mergeDictatedText } from "./dictation";

describe("mergeDictatedText", () => {
  it("inserts into an empty draft without leading space", () => {
    expect(mergeDictatedText("", 0, 0, "hello world")).toEqual({
      value: "hello world",
      cursor: 11,
    });
  });

  it("adds spacing between existing words", () => {
    expect(mergeDictatedText("helloagain", 5, 5, "brave")).toEqual({
      value: "hello brave again",
      cursor: 11,
    });
  });

  it("replaces the selection and keeps existing whitespace", () => {
    expect(mergeDictatedText("hello old world", 6, 9, "new")).toEqual({
      value: "hello new world",
      cursor: 9,
    });
  });

  it("ignores an empty transcript", () => {
    expect(mergeDictatedText("draft", 5, 5, "   ")).toEqual({
      value: "draft",
      cursor: 5,
    });
  });
});

describe("formatDictationTime", () => {
  it("formats minutes and padded seconds", () => {
    expect(formatDictationTime(7)).toBe("0:07");
    expect(formatDictationTime(75)).toBe("1:15");
  });
});
