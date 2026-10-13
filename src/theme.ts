import type { ThemeMode } from "./types";

/** "system" follows Windows (prefers-color-scheme); light/dark override it. */
export function applyTheme(mode: ThemeMode | undefined): void {
  const root = document.documentElement;
  if (mode === "light" || mode === "dark") root.dataset.theme = mode;
  else delete root.dataset.theme;
}
