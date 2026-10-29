// Settings — live-session trigger + AI client configuration.
// Owns its modal HTML partial, persists the char-limit and the AI API key to
// localStorage, and routes the non-secret AI config to the Rust-side store.
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";
import { loadAiConfig, loadStoredApiKey, saveAiConfig, testAiConnection } from "./ai";
import { initAppearance } from "./app/theme";
import { ensureModel } from "./features/model-setup";
import { showWalkthrough } from "./features/walkthrough";
import { refreshIcons } from "./lib/icons";
import { describeError } from "./lib/status";
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
let syncAppearance: () => void;
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
  // About and Usage are read-only; Appearance applies live.
  document.getElementById("settings-actions")!.hidden = ["about", "appearance", "usage"].includes(id);
  if (id === "about") void loadAbout();
  if (id === "llm") void loadSttModel();
  if (id === "usage") void loadUsage();
}

interface SessionUsage {
  session_id: number | null;
  title: string;
  started_at: string | null;
  calls: number;
  tokens_in: number;
  tokens_out: number;
}

const utcMonth = (d: Date) => d.toISOString().slice(0, 7);
/** Month shown in the Usage tab, `YYYY-MM` (UTC, as the DB groups it). */
let usageMonth = utcMonth(new Date());
/** Earliest month with usage; the back arrow stops here. */
let firstUsageMonth = usageMonth;

/// Move the Usage tab by `delta` months, never past the current one.
function stepUsageMonth(delta: number): void {
  const [y, m] = usageMonth.split("-").map(Number);
  const next = utcMonth(new Date(Date.UTC(y, m - 1 + delta, 1)));
  if (next > utcMonth(new Date()) || next < firstUsageMonth) return;
  usageMonth = next;
  void loadUsage();
}

/// Fill the Usage table for `usageMonth`.
async function loadUsage(): Promise<void> {
  const [y, m] = usageMonth.split("-").map(Number);
  document.getElementById("usage-month")!.textContent = new Date(Date.UTC(y, m - 1, 1)).toLocaleDateString([], {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
  firstUsageMonth = (await invoke<string | null>("usage_first_month").catch(() => null)) ?? utcMonth(new Date());
  (document.getElementById("usage-prev") as HTMLButtonElement).disabled = usageMonth <= firstUsageMonth;
  (document.getElementById("usage-next") as HTMLButtonElement).disabled = usageMonth >= utcMonth(new Date());
  const rows = await invoke<SessionUsage[]>("usage_report", { month: usageMonth }).catch(() => []);
  const sum = (f: (r: SessionUsage) => number) => rows.reduce((n, r) => n + f(r), 0);
  const n = (v: number) => v.toLocaleString();
  const total = document.getElementById("usage-total")!;
  total.replaceChildren();
  if (rows.length) {
    const strong = document.createElement("strong");
    strong.textContent = `${n(sum((r) => r.tokens_in + r.tokens_out))} tokens`;
    total.append(strong, ` · ${rows.length} ${rows.length === 1 ? "session" : "sessions"}`);
  } else {
    total.textContent = "No usage recorded for this month.";
  }
  const body = document.getElementById("usage-rows")!;
  body.replaceChildren(
    ...rows.map((r) => {
      const tr = document.createElement("tr");
      const name = r.session_id === null ? "Other / deleted session" : r.title || `Session ${r.session_id}`;
      const when = r.started_at
        ? new Date(r.started_at).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })
        : "";
      const first = document.createElement("td");
      first.className = "usage-session";
      const title = document.createElement("span");
      title.className = "usage-title";
      title.textContent = name;
      title.title = name;
      const date = document.createElement("span");
      date.className = "usage-when";
      date.textContent = when;
      first.append(title, date);
      tr.append(first);
      [r.tokens_in, r.tokens_out, r.tokens_in + r.tokens_out].forEach((v, i) => {
        const td = document.createElement("td");
        td.className = i === 2 ? "num usage-sum" : "num";
        td.textContent = n(v);
        tr.append(td);
      });
      return tr;
    }),
  );
}

async function loadAbout(): Promise<void> {
  const set = (id: string, text: string) => (document.getElementById(id)!.textContent = text);
  const [version, model, ready] = await Promise.all([
    getVersion(),
    invoke<string>("selected_model"),
    invoke<boolean>("models_ready"),
  ]);
  set("about-version", version);
  document.getElementById("update-btn")!.onclick = () => void onUpdateClick();
  set("about-model", `${model} (${ready ? "downloaded" : "not downloaded"})`);
}

let pendingUpdate: Update | null = null;

/// First click checks the release feed; once an update is found the same button installs it and restarts.
async function onUpdateClick(): Promise<void> {
  const status = document.getElementById("update-status")!;
  const button = document.getElementById("update-btn") as HTMLButtonElement;
  button.disabled = true;
  try {
    if (!pendingUpdate) {
      status.textContent = "Checking…";
      pendingUpdate = await check();
      status.textContent = pendingUpdate ? `Version ${pendingUpdate.version} is available.` : "You're up to date.";
      if (pendingUpdate) button.textContent = "Install and restart";
    } else {
      status.textContent = "Downloading…";
      await pendingUpdate.downloadAndInstall();
      await relaunch();
    }
  } catch (err) {
    status.textContent = `Update failed: ${describeError(err)}`;
  } finally {
    button.disabled = false;
  }
}

/// Show which on-device speech model transcribes calls, with a download button when it is missing.
async function loadSttModel(): Promise<void> {
  const status = document.getElementById("stt-status")!;
  const button = document.getElementById("stt-download") as HTMLButtonElement;
  const [id, models, ready] = await Promise.all([
    invoke<string>("selected_model"),
    invoke<{ id: string; label: string }[]>("list_models"),
    invoke<boolean>("models_ready"),
  ]);
  document.getElementById("stt-model")!.textContent = models.find((m) => m.id === id)?.label ?? id;
  status.dataset.state = ready ? "ok" : "error";
  status.textContent = ready ? "Downloaded" : "Not downloaded";
  button.hidden = ready;
  button.disabled = false;
  button.textContent = "Download";
}

async function downloadSttModel(): Promise<void> {
  const status = document.getElementById("stt-status")!;
  const button = document.getElementById("stt-download") as HTMLButtonElement;
  button.disabled = true;
  status.dataset.state = "busy";
  status.textContent = "Downloading…";
  const off = await listen<{ downloaded: number; total: number }>("model-progress", ({ payload: { downloaded, total } }) => {
    if (total > 0) status.textContent = `Downloading… ${Math.round((downloaded / total) * 100)}%`;
  });
  try {
    await invoke("download_model");
    await ensureModel();
    await loadSttModel();
  } catch (err) {
    status.dataset.state = "error";
    status.textContent = describeError(err);
    button.disabled = false;
    button.textContent = "Retry";
  } finally {
    off();
  }
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
  const box = document.getElementById("persona-summary") as HTMLTextAreaElement;
  const count = document.getElementById("persona-summary-count")!;
  const showCount = () => (count.textContent = `${box.value.length} / ${box.maxLength}`);
  let cur: Persona = { roleId: null, meetings: [] };
  // Once the user types in the box, role changes stop overwriting it.
  let edited = false;
  const generate = (p: Persona) =>
    invoke<string>("describe_persona", { persona: { ...p, summary: "" } }).then((t) => t.slice(0, box.maxLength));

  box.value = "";
  showCount();
  box.oninput = () => {
    edited = true;
    personaDraft = cur = { ...cur, summary: box.value };
    showCount();
  };
  loadPersona()
    .then(async (p) => {
      cur = p ?? cur;
      box.value = cur.summary || (await generate(cur));
      showCount();
      mountPersonaPicker(root, cur, async (picked) => {
        cur = { ...picked, summary: box.value };
        if (!edited) {
          box.value = await generate(cur).catch(() => box.value);
          cur.summary = box.value;
          showCount();
        }
        personaDraft = cur;
        setDirty(true);
      });
    })
    .catch((err) => (root.textContent = String(err)));
}

/// Open the settings modal, seeding both char-limit and AI config.
export function openSettings(): void {
  const settings = loadLiveSessionSettings();
  input.value = String(settings.charLimit);
  maxOpenInput.value = String(settings.maxOpenQuestions);
  notesWindowInput.value = String(settings.notesWindowMins);
  syncAppearance();
  usageMonth = utcMonth(new Date());
  showPane("persona");
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
  syncAppearance = initAppearance();
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
  document.getElementById("usage-prev")!.addEventListener("click", () => stepUsageMonth(-1));
  document.getElementById("usage-next")!.addEventListener("click", () => stepUsageMonth(1));
  document.getElementById("settings-close")!.addEventListener("click", closeSettings);
  document.getElementById("settings-cancel")!.addEventListener("click", closeSettings);
  document.getElementById("settings-save")!.addEventListener("click", saveSettings);
  aiTestBtn.addEventListener("click", () => void runAiTest());
  // Tauri's webview won't follow external links itself; route them through the opener.
  const website = document.getElementById("about-website") as HTMLAnchorElement;
  website.addEventListener("click", (e) => {
    e.preventDefault();
    void openUrl(website.href);
  });
  document.getElementById("about-guide")!.addEventListener("click", showWalkthrough);
  document.getElementById("stt-download")!.addEventListener("click", () => void downloadSttModel());
  modal.addEventListener("click", (e) => {
    if (e.target === modal) closeSettings();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modal.classList.contains("open")) closeSettings();
  });
}

init();
