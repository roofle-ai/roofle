// Settings — Live Session Settings (glossary) + AI client configuration.
// Owns its modal HTML partial, persists the char-limit and the AI API key to
// localStorage, and routes the non-secret AI config to the Rust-side store.
import { loadAiConfig, loadStoredApiKey, saveAiConfig, testAiConnection } from "./ai";
import { refreshIcons } from "./lib/icons";
import settingsHtml from "./ui/partials/settings.html?raw";

export interface PersonInfo {
  name: string;
  persona: string;
}

const PERSON_KEY = "roofle-person-info";

export interface LiveSessionSettings {
  charLimit: number;
}

const KEY = "roofle-live-session-settings";
export const DEFAULT_CHAR_LIMIT = 200;
const MIN_CHAR_LIMIT = 50;
const MAX_CHAR_LIMIT = 5000;

let modal: HTMLElement;
let input: HTMLInputElement;
let personName: HTMLInputElement;
let personPersona: HTMLSelectElement;
let aiBaseUrl: HTMLInputElement;
let aiModel: HTMLInputElement;
let aiApiKey: HTMLInputElement;
let aiKeyState: HTMLSpanElement;
let aiTestResult: HTMLSpanElement;
let aiTestBtn: HTMLButtonElement;

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

/// Read the saved person info (name + persona); empty strings when unset.
export function loadPersonInfo(): PersonInfo {
  try {
    const parsed = JSON.parse(localStorage.getItem(PERSON_KEY) ?? "{}") as Partial<PersonInfo>;
    return { name: parsed.name?.trim() ?? "", persona: parsed.persona ?? "" };
  } catch {
    return { name: "", persona: "" };
  }
}

function showPane(id: string): void {
  modal.querySelectorAll<HTMLElement>(".settings-nav-item").forEach((el) => {
    const on = el.dataset.pane === id;
    el.classList.toggle("active", on);
    el.setAttribute("aria-selected", String(on));
  });
  modal.querySelectorAll<HTMLElement>(".settings-pane").forEach((el) => {
    el.classList.toggle("active", el.dataset.pane === id);
  });
}

/// Seed the AI config fields from the Rust-side store.
function loadAiConfigFields(): void {
  aiTestResult.textContent = "";
  loadAiConfig()
    .then((cfg) => {
      aiBaseUrl.value = cfg.base_url;
      aiModel.value = cfg.model;
      aiApiKey.value = "";
      aiKeyState.textContent = cfg.has_api_key
        ? "API key saved."
        : "No API key saved.";
      aiTestBtn.disabled = !cfg.has_api_key && !aiApiKey.value;
    })
    .catch((err) => {
      aiKeyState.textContent = String(err);
    });
}

/// Persist the AI config fields to the Rust-side store.
async function saveAiConfigFields(): Promise<void> {
  await saveAiConfig(aiBaseUrl.value.trim(), aiModel.value.trim(), aiApiKey.value);
}

/// Verify the configured client with a trivial prompt.
async function runAiTest(): Promise<void> {
  aiTestBtn.disabled = true;
  aiTestResult.textContent = "Testing…";
  try {
    await saveAiConfigFields();
    const reply = await testAiConnection(loadStoredApiKey());
    aiTestResult.textContent = `OK — model replied: ${reply}`;
  } catch (err) {
    aiTestResult.textContent = `Failed: ${String(err)}`;
  } finally {
    aiTestBtn.disabled = false;
  }
}

/// Open the settings modal, seeding both char-limit and AI config.
export function openSettings(): void {
  input.value = String(loadLiveSessionSettings().charLimit);
  const person = loadPersonInfo();
  personName.value = person.name;
  personPersona.value = person.persona;
  showPane("person");
  loadAiConfigFields();
  modal.classList.add("open");
}

function closeSettings(): void {
  modal.classList.remove("open");
}

function saveSettings(): void {
  const charLimit = clamp(parseInt(input.value, 10) || DEFAULT_CHAR_LIMIT);
  localStorage.setItem(KEY, JSON.stringify({ charLimit } satisfies LiveSessionSettings));
  localStorage.setItem(
    PERSON_KEY,
    JSON.stringify({ name: personName.value.trim(), persona: personPersona.value } satisfies PersonInfo),
  );
  saveAiConfigFields()
    .catch((err) => {
      aiKeyState.textContent = String(err);
    })
    .finally(closeSettings);
}

function init(): void {
  document.body.insertAdjacentHTML("beforeend", settingsHtml);

  modal = document.getElementById("settings-modal")!;
  input = document.getElementById("char-limit") as HTMLInputElement;
  personName = document.getElementById("person-name") as HTMLInputElement;
  personPersona = document.getElementById("person-persona") as HTMLSelectElement;
  modal.querySelectorAll<HTMLElement>(".settings-nav-item").forEach((el) =>
    el.addEventListener("click", () => showPane(el.dataset.pane!)),
  );
  refreshIcons();
  aiBaseUrl = document.getElementById("ai-base-url") as HTMLInputElement;
  aiModel = document.getElementById("ai-model") as HTMLInputElement;
  aiApiKey = document.getElementById("ai-api-key") as HTMLInputElement;
  aiKeyState = document.getElementById("ai-key-state") as HTMLSpanElement;
  aiTestResult = document.getElementById("ai-test-result") as HTMLSpanElement;
  aiTestBtn = document.getElementById("ai-test") as HTMLButtonElement;

  document.getElementById("settings-close")!.addEventListener("click", closeSettings);
  document.getElementById("settings-cancel")!.addEventListener("click", closeSettings);
  document.getElementById("settings-save")!.addEventListener("click", saveSettings);
  aiTestBtn.addEventListener("click", () => void runAiTest());
  modal.addEventListener("click", (e) => {
    if (e.target === modal) closeSettings();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modal.classList.contains("open")) closeSettings();
  });
}

init();
