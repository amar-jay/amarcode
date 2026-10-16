export const RELEASES_URL = "https://github.com/amar-jay/amarcode/releases";
export const RELEASE_API =
  "https://api.github.com/repos/amar-jay/amarcode/releases/latest";

export type ReleaseAsset = { name: string; browser_download_url: string };
export type Installer = {
  os: "Windows" | "Linux" | "macOS";
  label: string;
  href: string;
};

export function parseAssets(assets: unknown): ReleaseAsset[] {
  if (!Array.isArray(assets)) return [];
  return assets.filter(
    (asset): asset is ReleaseAsset =>
      asset &&
      typeof asset.name === "string" &&
      typeof asset.browser_download_url === "string" &&
      asset.browser_download_url.startsWith(
        "https://github.com/amar-jay/amarcode/releases/download/",
      ),
  );
}

export function getInstallers(assets: ReleaseAsset[]): Installer[] {
  return assets
    .flatMap((asset): Installer[] => {
      const match = asset.name.match(/\.(exe|msi|AppImage|deb|rpm|dmg)$/i);
      if (!match) return [];
      const format = match[1];
      const os = /^(exe|msi)$/i.test(format)
        ? "Windows"
        : /^dmg$/i.test(format)
          ? "macOS"
          : "Linux";
      const architecture = /aarch64|arm64/i.test(asset.name)
        ? os === "macOS"
          ? "Apple silicon"
          : "ARM64"
        : /x86_64|amd64|x64/i.test(asset.name)
          ? "x64"
          : /universal/i.test(asset.name)
            ? "Universal"
            : "";
      return [
        {
          os,
          label: [architecture, `.${format}`].filter(Boolean).join(" · "),
          href: asset.browser_download_url,
        },
      ];
    })
    .sort((a, b) => a.os.localeCompare(b.os) || a.label.localeCompare(b.label));
}

let request: Promise<ReleaseAsset[]> | undefined;

export function fetchReleaseAssets() {
  // Both download controls share one request per page load.
  request ??= (async () => {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 8000);
    try {
      const response = await fetch(RELEASE_API, {
        signal: controller.signal,
        headers: { Accept: "application/vnd.github+json" },
      });
      if (!response.ok) throw new Error("Release lookup failed");
      const release = await response.json();
      return parseAssets(release.assets);
    } finally {
      clearTimeout(timeout);
    }
  })();
  return request;
}
