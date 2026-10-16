import { useEffect, useState } from "react";
import {
  useOperatingSystem,
  type Platform,
} from "@/hooks/use-operating-system";
import {
  RELEASES_URL,
  fetchReleaseAssets,
  parseAssets,
  getInstallers,
  type ReleaseAsset,
} from "@/lib/releases";

export { RELEASES_URL } from "@/lib/releases";
type Download = { href: string; label: string };
const fallback: Download = { href: RELEASES_URL, label: "Explore releases" };

export function selectDownload(assets: unknown, platform: Platform): Download {
  if (!platform.architecture) return fallback;
  const extension =
    platform.os === "Windows"
      ? /-setup\.exe$/i
      : platform.os === "macOS"
        ? /\.dmg$/i
        : /\.AppImage$/i;
  const architecture =
    platform.architecture === "arm64" ? /aarch64|arm64/i : /amd64|x86_64|x64/i;
  const asset = parseAssets(assets).find(
    (entry) => extension.test(entry.name) && architecture.test(entry.name),
  );
  return asset
    ? { href: asset.browser_download_url, label: `Download for ${platform.os}` }
    : fallback;
}

export function useDownload() {
  const platform = useOperatingSystem();
  const [release, setRelease] = useState<{
    assets: ReleaseAsset[];
    loading: boolean;
    failed: boolean;
  }>({ assets: [], loading: true, failed: false });
  useEffect(() => {
    let active = true;
    fetchReleaseAssets().then(
      (assets) => {
        if (active) setRelease({ assets, loading: false, failed: false });
      },
      () => {
        if (active) setRelease({ assets: [], loading: false, failed: true });
      },
    );
    return () => {
      active = false;
    };
  }, []);
  return {
    ...(platform ? selectDownload(release.assets, platform) : fallback),
    installers: getInstallers(release.assets),
    loading: release.loading,
    failed: release.failed,
  };
}
