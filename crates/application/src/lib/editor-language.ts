import type { Extension } from "@codemirror/state";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { yaml } from "@codemirror/lang-yaml";
import { sql } from "@codemirror/lang-sql";

export function languageForPath(path: string): Extension {
  const extension = path.split(".").pop()?.toLowerCase();
  switch (extension) {
    case "js":
    case "jsx":
    case "ts":
    case "tsx":
      return javascript({ jsx: true, typescript: extension.startsWith("ts") });
    case "json":
    case "jsonc":
      return json();
    case "html":
    case "htm":
      return html();
    case "css":
    case "scss":
      return css();
    case "md":
    case "mdx":
      return markdown();
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
      return [];
  }
}
