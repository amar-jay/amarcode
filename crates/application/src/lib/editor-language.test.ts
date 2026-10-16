import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import { languageDescriptionForPath, languageForPath } from "./editor-language";

describe("editor language support", () => {
  it.each([
    ["main.go", "Go"],
    ["src/main.cpp", "C++"],
    ["Program.cs", "C#"],
    ["Main.java", "Java"],
    ["index.php", "PHP"],
    ["app.rb", "Ruby"],
    ["main.swift", "Swift"],
    ["main.kt", "Kotlin"],
    ["main.lua", "Lua"],
    ["Cargo.toml", "TOML"],
    ["style.scss", "SCSS"],
    ["Dockerfile", "Dockerfile"],
    ["Dockerfile.dev", "Dockerfile"],
    ["Containerfile", "Dockerfile"],
    ["CMakeLists.txt", "CMake"],
    ["/home/user/.bashrc", "Shell"],
    ["C:\\project\\.zshrc", "Shell"],
  ])("detects and loads %s as %s", async (path, name) => {
    const description = languageDescriptionForPath(path);
    expect(description?.name).toBe(name);
    expect((await description!.load()).language).toBeDefined();
  });
  it("leaves unknown files as editable plain text", () => {
    expect(languageDescriptionForPath("file.unknown-language")).toBeNull();
    const state = EditorState.create({
      doc: "hello",
      extensions: languageForPath("file.unknown-language"),
    });
    expect(state.doc.toString()).toBe("hello");
  });
  it("loads a catalog parser that produces a real syntax tree", async () => {
    const support = await languageDescriptionForPath("main.go")!.load();
    const state = EditorState.create({
      doc: "package main\nfunc main() {}",
      extensions: support,
    });
    expect(ensureSyntaxTree(state, state.doc.length, 1000)).not.toBeNull();
    expect(syntaxTree(state).length).toBe(state.doc.length);
    expect(syntaxTree(state).topNode.name).not.toBe("");
  });
  it.each(["module.mjs", "module.cjs", "module.mts", "module.cts"])(
    "parses %s",
    (path) => {
      const state = EditorState.create({
        doc: "const value = 1;",
        extensions: languageForPath(path),
      });
      expect(syntaxTree(state).topNode.name).toBe("Script");
    },
  );
});
