import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  clipboardFilePaths,
  readClipboardAttachments,
} from "./clipboard-files";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function pathsFor(payloads: Record<string, string>) {
  return clipboardFilePaths((format) => payloads[format] ?? "");
}

describe("clipboard file paths", () => {
  it("decodes URI lists with comments, spaces, Unicode, and CRLF", () => {
    expect(
      pathsFor({
        "text/uri-list":
          "# files\r\nfile:///tmp/a%20photo.png\r\nfile:///tmp/%C3%A7.gif\r\nfile:///tmp/a%20photo.png\r\n",
      }),
    ).toEqual(["/tmp/a photo.png", "/tmp/ç.gif"]);
  });

  it.each(["copy", "cut"])(
    "reads a GNOME file-manager %s payload",
    (operation) => {
      expect(
        pathsFor({
          "x-special/gnome-copied-files": `${operation}\nfile:///tmp/image.png\nfile:///tmp/image.webp`,
        }),
      ).toEqual(["/tmp/image.png", "/tmp/image.webp"]);
    },
  );

  it("recognizes image paths and quoted Windows Copy-as-path text", () => {
    for (const [text, path] of [
      ["/home/user/Pictures/a photo.PNG", "/home/user/Pictures/a photo.PNG"],
      ['"C:\\Users\\Person\\a photo.jpg"', "C:\\Users\\Person\\a photo.jpg"],
      [
        "file:///C:/Users/Person/a%20photo.webp",
        "C:/Users/Person/a photo.webp",
      ],
      ["file://localhost/tmp/image.gif", "/tmp/image.gif"],
    ]) {
      expect(pathsFor({ "text/plain": text })).toEqual([path]);
    }
  });

  it("preserves ordinary text, URLs, and prose that mentions images", () => {
    for (const text of [
      "ordinary text",
      "Please inspect /tmp/a.png",
      "/tmp/a.png\nthen change the button",
      "https://example.com/a.png",
      "file://remote-host/tmp/a.png",
      "file:///tmp/bad%zz.png",
      "/tmp/readme.txt",
      "",
    ]) {
      expect(pathsFor({ "text/plain": text })).toEqual([]);
    }
  });
});

describe("clipboard attachments", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("attaches an image copied as a path without trying the screenshot reader", async () => {
    vi.mocked(invoke).mockResolvedValue({
      images: [
        {
          filename: "a photo.png",
          media_type: "image/png",
          data: "iVBORw0KGgo=",
        },
      ],
      fallback_paths: [],
    });
    const paths = pathsFor({ "text/plain": "/tmp/a photo.png" });
    const result = await readClipboardAttachments(paths);
    expect(invoke).toHaveBeenCalledWith("read_pasted_files", { paths });
    expect(result.files[0].name).toBe("a photo.png");
    expect(result.files[0].type).toBe("image/png");
    expect([...new Uint8Array(await result.files[0].arrayBuffer())]).toEqual([
      137, 80, 78, 71, 13, 10, 26, 10,
    ]);
    expect(result.fallbackPaths).toEqual([]);
  });

  it("reads native file lists even when the webview exposes no clipboard payload", async () => {
    vi.mocked(invoke).mockResolvedValue({
      images: [],
      fallback_paths: ["/tmp/unsupported.bmp"],
    });
    expect(await readClipboardAttachments([])).toEqual({
      files: [],
      fallbackPaths: ["/tmp/unsupported.bmp"],
    });
  });

  it("retains paths if the native file reader fails", async () => {
    vi.mocked(invoke).mockRejectedValue("unavailable");
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    const result = await readClipboardAttachments(["/tmp/missing.png"]);
    expect(result).toEqual({ files: [], fallbackPaths: ["/tmp/missing.png"] });
    warning.mockRestore();
  });

  it("attaches a screenshot encoded by the native worker", async () => {
    vi.mocked(invoke).mockResolvedValue({
      images: [
        {
          filename: "clipboard.png",
          media_type: "image/png",
          data: "iVBORw0KGgo=",
        },
      ],
      fallback_paths: [],
    });
    const result = await readClipboardAttachments([]);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("read_pasted_files", {
      paths: [],
    });
    expect(result.files[0].name).toBe("clipboard.png");
    expect(result.files[0].type).toBe("image/png");
    expect([...new Uint8Array(await result.files[0].arrayBuffer())]).toEqual([
      137, 80, 78, 71, 13, 10, 26, 10,
    ]);
    expect(result.fallbackPaths).toEqual([]);
  });

  it("returns no attachments for an empty clipboard", async () => {
    vi.mocked(invoke).mockResolvedValue({ images: [], fallback_paths: [] });
    expect(await readClipboardAttachments([])).toEqual({
      files: [],
      fallbackPaths: [],
    });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
