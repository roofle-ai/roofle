import { storeGet, storeSet } from "../lib/storage";

type Theme = "system" | "light" | "dark";

const THEME_KEY = "roofle-theme";
const OPACITY_KEY = "roofle-opacity";

const dark = window.matchMedia("(prefers-color-scheme: dark)");

/** "system" resolves to light/dark here so the explicit data-theme CSS keeps working. */
function applyTheme(theme: Theme): void {
  const resolved = theme === "system" ? (dark.matches ? "dark" : "light") : theme;
  document.documentElement.setAttribute("data-theme", resolved);
  storeSet(THEME_KEY, theme);
}

dark.addEventListener("change", () => {
  if (currentTheme() === "system") applyTheme("system");
});

function currentTheme(): Theme {
  const t = storeGet(THEME_KEY);
  return t === "light" || t === "dark" ? t : "system";
}

function applyOpacity(pct: number): void {
  document.documentElement.style.setProperty("--opacity", String(pct / 100));
  storeSet(OPACITY_KEY, String(pct));
}

export function initTheme(): void {
  applyTheme(currentTheme());
  applyOpacity(Number(storeGet(OPACITY_KEY)) || 100);
}

/** Wire the Appearance pane controls (called once the settings modal exists). */
export function initAppearance(): () => void {
  const opts = document.querySelectorAll<HTMLButtonElement>("[data-theme-opt]");
  const range = document.getElementById("opacity-range") as HTMLInputElement;
  const label = document.getElementById("opacity-value")!;
  const sync = () => {
    opts.forEach((o) => o.setAttribute("aria-pressed", String(o.dataset.themeOpt === currentTheme())));
    range.value = String(Number(storeGet(OPACITY_KEY)) || 100);
    label.textContent = `${range.value}%`;
  };
  opts.forEach((o) =>
    o.addEventListener("click", () => {
      applyTheme(o.dataset.themeOpt as Theme);
      sync();
    }),
  );
  range.addEventListener("input", () => {
    applyOpacity(Number(range.value));
    label.textContent = `${range.value}%`;
  });
  return sync;
}
