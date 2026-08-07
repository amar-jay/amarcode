import { useEffect, useState } from "react"

export type Theme = "light" | "dark" | "system"

function readTheme(): Theme {
  const stored = localStorage.getItem("acp-workbench-theme")
  return stored === "light" || stored === "dark" || stored === "system" ? stored : "system"
}

export function useTheme() {
  const [theme, setTheme] = useState<Theme>(readTheme)

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)")
    const apply = () => document.documentElement.classList.toggle("dark", theme === "dark" || (theme === "system" && media.matches))
    apply()
    localStorage.setItem("acp-workbench-theme", theme)
    media.addEventListener("change", apply)
    return () => media.removeEventListener("change", apply)
  }, [theme])

  return { theme, setTheme }
}
