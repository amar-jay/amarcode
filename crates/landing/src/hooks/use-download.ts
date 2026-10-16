import { useEffect, useState } from "react";
import {
  useOperatingSystem,
  type Platform,
} from "@/hooks/use-operating-system";

export const RELEASES_URL = "https://github.com/amar-jay/amarcode/releases";
const RELEASE_API =
  "https://api.github.com/repos/amar-jay/amarcode/releases/latest";
type Download = { href: string; label: string };
const fallback: Download = { href: RELEASES_URL, label: "Explore releases" };

export function selectDownload(assets: unknown, platform: Platform): Download {
  if (!Array.isArray(assets) || !platform.architecture) return fallback;
  const extension =
    platform.os === "Windows"
      ? /-setup\.exe$/i
      : platform.os === "macOS"
        ? /\.dmg$/i
        : /\.AppImage$/i;
  const architecture =
    platform.architecture === "arm64" ? /aarch64|arm64/i : /amd64|x86_64|x64/i;
  const asset = assets.find(
    (entry) =>
      entry &&
      typeof entry.name === "string" &&
      typeof entry.browser_download_url === "string" &&
      extension.test(entry.name) &&
      architecture.test(entry.name) &&
      entry.browser_download_url.startsWith(
        "https://github.com/amar-jay/amarcode/releases/download/",
      ),
  );
  return asset
    ? { href: asset.browser_download_url, label: `Download for ${platform.os}` }
    : fallback;
}

export function useDownload() {
  const platform = useOperatingSystem();
  const [download, setDownload] = useState<Download>(fallback);
  useEffect(() => {
    if (!platform) return;
    const controller = new AbortController();
    let active = true;
    const timeout = window.setTimeout(() => controller.abort(), 8000);
    async function resolveDownload() {
      try {
        const response = await fetch(RELEASE_API, {
          signal: controller.signal,
          headers: { Accept: "application/vnd.github+json" },
        });
        if (!response.ok) return;
        const release = await response.json();
        if (active) setDownload(selectDownload(release.assets, platform!));
      } catch {
        // The releases page remains usable when the lookup fails or is rate-limited.
      } finally {
        window.clearTimeout(timeout);
      }
    }
    void resolveDownload();
    return () => {
      active = false;
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [platform]);
  return download;
}
