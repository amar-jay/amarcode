import { useState } from "react";

export type OperatingSystem = "Windows" | "macOS" | "Linux";
export type Platform = {
  os: OperatingSystem;
  architecture: "arm64" | "x64" | null;
};

export function detectPlatform(
  userAgent: string,
  platform: string,
  touchPoints = 0,
): Platform | null {
  if (
    /Android|iPhone|iPad|iPod|CrOS/i.test(userAgent) ||
    (/Mac/i.test(platform) && touchPoints > 1)
  )
    return null;
  const identity = `${userAgent} ${platform}`;
  const os = /Win/i.test(platform)
    ? "Windows"
    : /Mac/i.test(platform)
      ? "macOS"
      : /Linux/i.test(platform)
        ? "Linux"
        : null;
  if (!os) return null;
  const architecture = /aarch64|arm64/i.test(identity)
    ? "arm64"
    : /x86_64|x64|Win64|amd64/i.test(identity)
      ? "x64"
      : null;
  return { os, architecture };
}

export function useOperatingSystem() {
  const [platform] = useState<Platform | null>(() =>
    typeof navigator === "undefined"
      ? null
      : detectPlatform(
          navigator.userAgent,
          navigator.platform,
          navigator.maxTouchPoints,
        ),
  );
  return platform;
}
