/**
 * @deprecated Prefer `@/state` theme/palette atoms.
 * Kept as a thin re-export so existing imports keep working.
 */
export type { Theme, Palette } from "@/state/preferences";
export { readPalette, themeAtom, paletteAtom } from "@/state/preferences";

import { useAtom } from "jotai";
import { paletteAtom, themeAtom } from "@/state/preferences";

export function useTheme() {
  const [theme, setTheme] = useAtom(themeAtom);
  const [palette, setPalette] = useAtom(paletteAtom);
  return { theme, setTheme, palette, setPalette };
}
