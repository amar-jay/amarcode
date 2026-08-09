import { useEffect, useState } from "react";

export type Theme = "light" | "dark" | "system";
export type Palette = "monochrome" | "ember";

function readTheme(): Theme {
  const stored = localStorage.getItem("amarcode-theme");
  return stored === "light" || stored === "dark" || stored === "system"
    ? stored
    : "system";
}

export function readPalette(): Palette {
  const stored = localStorage.getItem("amarcode-palette");
  return stored === "ember" || stored === "monochrome"
    ? stored
    : "monochrome";
}

export function useTheme() {
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [palette, setPalette] = useState<Palette>(readPalette);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () =>
      document.documentElement.classList.toggle(
        "dark",
        theme === "dark" || (theme === "system" && media.matches),
      );
    apply();
    localStorage.setItem("amarcode-theme", theme);
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.style = palette;
    localStorage.setItem("amarcode-palette", palette);
  }, [palette]);

  return { theme, setTheme, palette, setPalette };
}
