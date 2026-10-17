// Settings — live-session trigger + AI client configuration.
// Owns its modal HTML partial, persists the char-limit and the AI API key to
// localStorage, and routes the non-secret AI config to the Rust-side store.
import { invoke } from "@tauri-apps/api/core";
import { getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { loadAiConfig, loadStoredApiKey, saveAiConfig, testAiConnection } from "./ai";
import { refreshIcons } from "./lib/icons";
import { loadPersona, mountPersonaPicker, savePersona, type Persona } from "./persona";
import settingsHtml from "./ui/partials/settings.html?raw";

export interface LiveSessionSettings {
  charLimit: number;
  /** Most follow-up questions shown at once. */
  maxOpenQuestions: number;
  /** Minutes of conversation between note updates. */
  notesWindowMins: number;
}

const KEY = "roofle-live-session-settings";
export const DEFAULT_CHAR_LIMIT = 500;
const MIN_CHAR_LIMIT = 50;
const MAX_CHAR_LIMIT = 5000;
export const DEFAULT_MAX_OPEN = 2;
const MAX_OPEN_CAP = 5;
export const DEFAULT_NOTES_WINDOW = 5;
const MAX_NOTES_WINDOW = 60;

/** Set only when the user changes the persona, so untouched settings never write one. */
let personaDraft: Persona | null = null;
let saveBtn: HTMLButtonElement;
let statusEl: HTMLElement;
let modal: HTMLElement;
let input: HTMLInputElement;
let maxOpenInput: HTMLInputElement;
let notesWindowInput: HTMLInputElement;
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
      notesWindowMins: clampNotesWindow(Number(parsed.notesWindowMins)),
    };
  } catch {
    return { charLimit: DEFAULT_CHAR_LIMIT, maxOpenQuestions: DEFAULT_MAX_OPEN, notesWindowMins: DEFAULT_NOTES_WINDOW };
  }
}

function clampNotesWindow(n: number): number {
  return Math.min(MAX_NOTES_WINDOW, Math.max(1, Math.round(n) || DEFAULT_NOTES_WINDOW));
}

/** Save stays disabled until something changes, so it is clear when there is nothing to save. */
function setDirty(on: boolean): void {
  saveBtn.disabled = !on;
  statusEl.textContent = on ? "Unsaved changes" : "";
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
  // Nothing to save on About.
  document.getElementById("settings-actions")!.hidden = id === "about";
  if (id === "about") void loadAbout();
}

async function loadAbout(): Promise<void> {
  const set = (id: string, text: string) => (document.getElementById(id)!.textContent = text);
  const [version, tauri, model, ready] = await Promise.all([
    getVersion(),
    getTauriVersion(),
    invoke<string>("selected_model"),
    invoke<boolean>("models_ready"),
  ]);
  set("about-version", version);
  set("about-tauri", `Tauri ${tauri}`);
  set("about-model", `${model} (${ready ? "downloaded" : "not downloaded"})`);
}

/// Seed the AI config fields from the Rust-side store.
function loadAiConfigFields(): void {
  aiTestResult.textContent = "";
  delete aiTestResult.dataset.state;
  loadAiConfig()
    .then((cfg) => {
      aiBaseUrl.value = cfg.base_url;
      aiModel.value = cfg.model;
      aiApiKey.value = "";
      aiKeyState.textContent = cfg.has_api_key
        ? "API key saved. Leave blank to keep it."
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
  aiTestResult.dataset.state = "busy";
  aiTestResult.textContent = "Testing…";
  try {
    await saveAiConfigFields();
    const reply = await testAiConnection(loadStoredApiKey());
    aiTestResult.dataset.state = "ok";
    aiTestResult.textContent = `Connected. Model replied: ${reply}`;
  } catch (err) {
    aiTestResult.dataset.state = "error";
    aiTestResult.textContent = String(err).replace(/^Error:\s*/, "");
  } finally {
    aiTestBtn.disabled = false;
  }
}

function loadPersonaPane(): void {
  personaDraft = null;
  const root = document.getElementById("settings-persona-picker")!;
  loadPersona()
    .then((p) => mountPersonaPicker(root, p ?? { roleId: null, meetings: [] }, (v) => ((personaDraft = v), setDirty(true))))
    .catch((err) => (root.textContent = String(err)));
}

/// Open the settings modal, seeding both char-limit and AI config.
export function openSettings(): void {
  const settings = loadLiveSessionSettings();
  input.value = String(settings.charLimit);
  maxOpenInput.value = String(settings.maxOpenQuestions);
  notesWindowInput.value = String(settings.notesWindowMins);
  showPane("llm");
  loadAiConfigFields();
  loadPersonaPane();
  aiApiKey.type = "password";
  document.getElementById("ai-key-toggle")!.textContent = "Show";
  setDirty(false);
  modal.classList.add("open");
}

function closeSettings(): void {
  modal.classList.remove("open");
}

function saveSettings(): void {
  const charLimit = clamp(parseInt(input.value, 10) || DEFAULT_CHAR_LIMIT);
  const maxOpenQuestions = clampMaxOpen(parseInt(maxOpenInput.value, 10));
  const notesWindowMins = clampNotesWindow(parseInt(notesWindowInput.value, 10));
  localStorage.setItem(KEY, JSON.stringify({ charLimit, maxOpenQuestions, notesWindowMins } satisfies LiveSessionSettings));
  saveAiConfigFields()
    .then(() => personaDraft && savePersona(personaDraft))
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
  notesWindowInput = document.getElementById("notes-window") as HTMLInputElement;
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

  saveBtn = document.getElementById("settings-save") as HTMLButtonElement;
  statusEl = document.getElementById("settings-status")!;
  modal.addEventListener("input", () => setDirty(true));
  document.getElementById("ai-key-toggle")!.addEventListener("click", (e) => {
    const show = aiApiKey.type === "password";
    aiApiKey.type = show ? "text" : "password";
    (e.currentTarget as HTMLElement).textContent = show ? "Hide" : "Show";
  });
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
