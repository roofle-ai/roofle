import { storeGet, storeSet } from "../lib/storage";

type Theme = "light" | "dark";

const THEME_KEY = "roofle-theme";
const OPACITY_KEY = "roofle-opacity";

function applyTheme(theme: Theme): void {
  document.documentElement.setAttribute("data-theme", theme);
  storeSet(THEME_KEY, theme);
}

function applyOpacity(pct: number): void {
  document.documentElement.style.setProperty("--opacity", String(pct / 100));
  storeSet(OPACITY_KEY, String(pct));
}

export function initTheme(): void {
  const saved = storeGet(THEME_KEY) as Theme | null;
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  applyTheme(saved ?? (systemDark ? "dark" : "light"));
  applyOpacity(Number(storeGet(OPACITY_KEY)) || 100);
}

/** Wire the Appearance pane controls (called once the settings modal exists). */
export function initAppearance(): () => void {
  const select = document.getElementById("theme-select") as HTMLSelectElement;
  const range = document.getElementById("opacity-range") as HTMLInputElement;
  const label = document.getElementById("opacity-value")!;
  const sync = () => {
    select.value = document.documentElement.getAttribute("data-theme") ?? "light";
    range.value = String(Number(storeGet(OPACITY_KEY)) || 100);
    label.textContent = `${range.value}%`;
  };
  select.addEventListener("change", () => applyTheme(select.value as Theme));
  range.addEventListener("input", () => {
    applyOpacity(Number(range.value));
    label.textContent = `${range.value}%`;
  });
  return sync;
}
