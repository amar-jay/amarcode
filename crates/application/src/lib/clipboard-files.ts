import { invoke } from "@tauri-apps/api/core";

const IMAGE_PATH = /\.(?:png|jpe?g|webp|gif|bmp|tiff?|heic|heif|avif|svg)$/i;
const ABSOLUTE_PATH = /^(?:\/|[a-z]:[\\/]|\\\\)/i;

function filePath(value: string): string | null {
  if (!/^file:/i.test(value)) return null;
  try {
    const url = new URL(value);
    if (url.hostname && url.hostname !== "localhost") return null;
    const path = decodeURIComponent(url.pathname);
    // Windows file URLs include a slash before the drive letter.
    return /^\/[a-z]:\//i.test(path) ? path.slice(1) : path;
  } catch {
    return null;
  }
}

/** File managers expose URI lists; "Copy as path" exposes plain text. */
export function clipboardFilePaths(
  getData: (format: string) => string,
): string[] {
  for (const format of [
    "text/uri-list",
    "x-special/gnome-copied-files",
    "x-special/mate-copied-files",
  ]) {
    const paths = getData(format)
      .split(/\r?\n/)
      .map((line) => filePath(line.trim()))
      .filter((path): path is string => path !== null);
    if (paths.length) return [...new Set(paths)];
  }

  const lines = getData("text/plain").trim().split(/\r?\n/);
  const paths = lines.map((line) => {
    let value = line.trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    const path = /^file:/i.test(value) ? filePath(value) : value;
    return path && ABSOLUTE_PATH.test(path) && IMAGE_PATH.test(path)
      ? path
      : null;
  });
  // Do not consume ordinary prose or snippets that happen to mention an image.
  return paths.every((path) => path !== null) ? [...new Set(paths)] : [];
}

type PastedFiles = {
  images: { filename: string; media_type: string; data: string }[];
  fallback_paths: string[];
};

export type ClipboardAttachments = { files: File[]; fallbackPaths: string[] };

/** Read copied files first, then fall back to the existing screenshot reader. */
export async function readClipboardAttachments(
  paths: string[],
  readImage: () => Promise<File | null>,
): Promise<ClipboardAttachments> {
  try {
    const result = await invoke<PastedFiles>("read_pasted_files", { paths });
    if (result.images.length || result.fallback_paths.length) {
      return {
        files: result.images.map((image) => {
          const bytes = Uint8Array.from(atob(image.data), (char) =>
            char.charCodeAt(0),
          );
          return new File([bytes], image.filename, { type: image.media_type });
        }),
        fallbackPaths: result.fallback_paths,
      };
    }
  } catch (error) {
    console.warn("Unable to read copied files from the clipboard:", error);
  }

  if (paths.length) return { files: [], fallbackPaths: paths };
  const image = await readImage();
  return { files: image ? [image] : [], fallbackPaths: [] };
}
