// Settings — live-session trigger + AI client configuration.
// Owns its modal HTML partial, persists the char-limit and the AI API key to
// localStorage, and routes the non-secret AI config to the Rust-side store.
import { loadAiConfig, loadStoredApiKey, saveAiConfig, testAiConnection } from "./ai";
import { refreshIcons } from "./lib/icons";
import settingsHtml from "./ui/partials/settings.html?raw";

export interface LiveSessionSettings {
  charLimit: number;
  /** Most follow-up questions shown at once. */
  maxOpenQuestions: number;
}

const KEY = "roofle-live-session-settings";
export const DEFAULT_CHAR_LIMIT = 500;
const MIN_CHAR_LIMIT = 50;
const MAX_CHAR_LIMIT = 5000;
export const DEFAULT_MAX_OPEN = 2;
const MAX_OPEN_CAP = 5;

let modal: HTMLElement;
let input: HTMLInputElement;
let maxOpenInput: HTMLInputElement;
let aiBaseUrl: HTMLInputElement;
let aiModel: HTMLInputElement;
let aiApiKey: HTMLInputElement;
let aiKeyState: HTMLSpanElement;
let aiTestResult: HTMLSpanElement;
let aiTestBtn: HTMLButtonElement;

function clamp(n: number): number {
  return Math.min(MAX_CHAR_LIMIT, Math.max(MIN_CHAR_LIMIT, n));
}

function clampMaxOpen(n: number): number {
  return Math.min(MAX_OPEN_CAP, Math.max(1, Math.round(n) || DEFAULT_MAX_OPEN));
}

/// Read persisted live-session settings; defaults to a 500-char limit and 2 open questions.
export function loadLiveSessionSettings(): LiveSessionSettings {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<LiveSessionSettings>;
    return {
      charLimit: clamp(Number(parsed.charLimit) || DEFAULT_CHAR_LIMIT),
      maxOpenQuestions: clampMaxOpen(Number(parsed.maxOpenQuestions)),
    };
  } catch {
    return { charLimit: DEFAULT_CHAR_LIMIT, maxOpenQuestions: DEFAULT_MAX_OPEN };
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
  const settings = loadLiveSessionSettings();
  input.value = String(settings.charLimit);
  maxOpenInput.value = String(settings.maxOpenQuestions);
  showPane("llm");
  loadAiConfigFields();
  modal.classList.add("open");
}

function closeSettings(): void {
  modal.classList.remove("open");
}

function saveSettings(): void {
  const charLimit = clamp(parseInt(input.value, 10) || DEFAULT_CHAR_LIMIT);
  const maxOpenQuestions = clampMaxOpen(parseInt(maxOpenInput.value, 10));
  localStorage.setItem(KEY, JSON.stringify({ charLimit, maxOpenQuestions } satisfies LiveSessionSettings));
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
  maxOpenInput = document.getElementById("max-open") as HTMLInputElement;
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
