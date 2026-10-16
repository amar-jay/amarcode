import { Compartment, type Extension } from "@codemirror/state";
import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { ViewPlugin } from "@codemirror/view";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { yaml } from "@codemirror/lang-yaml";
import { sql } from "@codemirror/lang-sql";

/** Filename rules cover extensionless files such as Dockerfile and Makefile. */
export function languageDescriptionForPath(
  path: string,
): LanguageDescription | null {
  const filename = path.replaceAll("\\", "/").split("/").pop() ?? path;
  const alias = /^(?:Dockerfile|Containerfile)(?:\..*)?$/i.test(filename)
    ? "Dockerfile"
    : /^\.(?:bashrc|bash_profile|zshrc|zprofile)$/.test(filename)
      ? "script.sh"
      : filename;
  return LanguageDescription.matchFilename(languages, alias);
}

/** Load uncommon parsers without rebuilding the editor or losing its history. */
function catalogLanguage(path: string): Extension {
  const description = languageDescriptionForPath(path);
  if (!description) return [];
  if (description.support) return description.support;
  const language = new Compartment();
  return [
    language.of([]),
    ViewPlugin.define((view) => {
      let active = true;
      void description
        .load()
        .then((support) => {
          if (active) view.dispatch({ effects: language.reconfigure(support) });
        })
        .catch((error: unknown) => {
          // Text editing remains available if a parser chunk cannot be loaded.
          console.warn(
            `Could not load ${description.name} language support`,
            error,
          );
        });
      return {
        destroy() {
          active = false;
        },
      };
    }),
  ];
}

export function languageForPath(path: string): Extension {
  const extension = path.split(".").pop()?.toLowerCase();
  switch (extension) {
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
    case "mts":
    case "cts":
    case "ts":
    case "tsx":
      return javascript({
        jsx: extension === "jsx" || extension === "tsx",
        typescript: ["ts", "tsx", "mts", "cts"].includes(extension),
      });
    case "json":
    case "jsonc":
      return json();
    case "html":
    case "htm":
      return html();
    case "css":
      return css();
    case "md":
    case "mdx":
      return markdown({ codeLanguages: languages });
    case "rs":
      return rust();
    case "py":
      return python();
    case "yaml":
    case "yml":
      return yaml();
    case "sql":
      return sql();
    default:
      return catalogLanguage(path);
  }
}
