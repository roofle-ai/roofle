import { dom } from "../lib/dom";
import { refreshIcons } from "../lib/icons";
import { storeGet, storeSet } from "../lib/storage";

type Theme = "light" | "dark";

const THEME_KEY = "roofle-theme";

function applyTheme(theme: Theme): void {
  document.documentElement.setAttribute("data-theme", theme);
  dom.themeToggle.innerHTML = `<i data-lucide="${theme === "dark" ? "sun" : "moon"}" aria-hidden="true"></i><span>${theme === "dark" ? "Light mode" : "Dark mode"}</span>`;
  dom.themeToggle.setAttribute(
    "aria-label",
    theme === "dark" ? "Switch to light theme" : "Switch to dark theme",
  );
  refreshIcons();
  storeSet(THEME_KEY, theme);
}

function currentTheme(): Theme {
  return document.documentElement.getAttribute("data-theme") === "dark" ? "dark" : "light";
}

export function initTheme(): void {
  const saved = storeGet(THEME_KEY) as Theme | null;
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  applyTheme(saved ?? (systemDark ? "dark" : "light"));
  dom.themeToggle.addEventListener("click", () =>
    applyTheme(currentTheme() === "dark" ? "light" : "dark"),
  );
}
