import { describe, expect, test } from "bun:test";
import { getInstallers, parseAssets } from "../src/lib/releases";

const asset = (name: string) => ({
  name,
  browser_download_url: `https://github.com/amar-jay/amarcode/releases/download/latest/${name}`,
});

describe("release download menu", () => {
  test("includes installers and excludes signatures, updater bundles and manifests", () => {
    const assets = [
      "app_x64-setup.exe",
      "app_x64.msi",
      "app_amd64.AppImage",
      "app_amd64.deb",
      "app_x86_64.rpm",
      "app_aarch64.dmg",
      "app_x64.msi.sig",
      "app_amd64.AppImage.sig",
      "app.app.tar.gz",
      "latest.json",
    ].map(asset);
    const installers = getInstallers(parseAssets(assets));
    expect(installers).toHaveLength(6);
    expect(installers.filter((item) => item.os === "Windows")).toHaveLength(2);
    expect(installers.filter((item) => item.os === "Linux")).toHaveLength(3);
    expect(installers.find((item) => item.os === "macOS")?.label).toBe(
      "Apple silicon · .dmg",
    );
  });
  test("does not offer missing platforms", () => {
    expect(
      getInstallers([asset("app_x64-setup.exe")]).map((item) => item.os),
    ).toEqual(["Windows"]);
  });
  test("rejects malformed or unrelated download links", () => {
    expect(
      parseAssets([
        null,
        {},
        { name: "app.exe" },
        {
          ...asset("app.exe"),
          browser_download_url: "https://example.com/app.exe",
        },
      ]),
    ).toEqual([]);
    expect(parseAssets(undefined)).toEqual([]);
  });
  test("describes ARM builds correctly on Windows and Linux", () => {
    expect(
      getInstallers([
        asset("app_arm64-setup.exe"),
        asset("app_aarch64.deb"),
      ]).every((item) => item.label.startsWith("ARM64")),
    ).toBe(true);
  });
});
