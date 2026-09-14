// Settings — Live Session Settings (glossary) with a Triggers sub-section.
// Owns its modal HTML partial and persists the char-limit to localStorage.
import settingsHtml from "./ui/partials/settings.html?raw";

export interface LiveSessionSettings {
  charLimit: number;
}

const KEY = "roofle-live-session-settings";
export const DEFAULT_CHAR_LIMIT = 200;
const MIN_CHAR_LIMIT = 50;
const MAX_CHAR_LIMIT = 5000;

let modal: HTMLElement;
let input: HTMLInputElement;

function clamp(n: number): number {
  return Math.min(MAX_CHAR_LIMIT, Math.max(MIN_CHAR_LIMIT, n));
}

/// Read persisted live-session settings; defaults to a 200-char limit.
export function loadLiveSessionSettings(): LiveSessionSettings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { charLimit: DEFAULT_CHAR_LIMIT };
    const parsed = JSON.parse(raw) as Partial<LiveSessionSettings>;
    return { charLimit: clamp(Number(parsed.charLimit) || DEFAULT_CHAR_LIMIT) };
  } catch {
    return { charLimit: DEFAULT_CHAR_LIMIT };
  }
}

/// Open the settings modal, seeding the input from persisted settings.
export function openSettings(): void {
  input.value = String(loadLiveSessionSettings().charLimit);
  modal.classList.add("open");
}

function closeSettings(): void {
  modal.classList.remove("open");
}

function saveSettings(): void {
  const charLimit = clamp(parseInt(input.value, 10) || DEFAULT_CHAR_LIMIT);
  localStorage.setItem(KEY, JSON.stringify({ charLimit } satisfies LiveSessionSettings));
  closeSettings();
}

function init(): void {
  document.body.insertAdjacentHTML("beforeend", settingsHtml);

  modal = document.getElementById("settings-modal")!;
  input = document.getElementById("char-limit") as HTMLInputElement;

  document.getElementById("settings-close")!.addEventListener("click", closeSettings);
  document.getElementById("settings-cancel")!.addEventListener("click", closeSettings);
  document.getElementById("settings-save")!.addEventListener("click", saveSettings);
  modal.addEventListener("click", (e) => {
    if (e.target === modal) closeSettings();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modal.classList.contains("open")) closeSettings();
  });
}

init();
