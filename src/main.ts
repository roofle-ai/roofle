// Roofle frontend — minimal meeting copilot UI.
//
// A single transport control (Start / Pause / Resume / Stop) drives ONE
// recording session that captures both the microphone and system audio mixed
// into a single transcript, rendered as plain essay paragraphs. The Rust shell
// exposes start/pause/resume/stop commands and emits `transcript-update`,
// `level`, `model-progress`, `recording-state` and `app-status` events.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getSession, listSessions } from "./transcripts";

/// Payload of a `transcript-update` event (mirrors roofle_core::asr::Segment).
interface Segment {
  kind: "Partial" | "Final";
  text: string;
}

/// Payload of the `list_models` command.
interface ModelInfo {
  id: string;
  label: string;
  present: boolean;
}

/// Payload of a `model-progress` event.
interface ModelProgress {
  id: string;
  downloaded: number;
  total: number;
}

const recordBtn = document.querySelector<HTMLButtonElement>("#record-btn")!;
const recordIconPlay = document.querySelector<SVGElement>("#record-icon-play")!;
const recordIconPause = document.querySelector<SVGElement>("#record-icon-pause")!;
const stopBtn = document.querySelector<HTMLButtonElement>("#stop-btn")!;
const timerEl = document.querySelector<HTMLSpanElement>("#timer")!;
const modelSelect = document.querySelector<HTMLSelectElement>("#model-select")!;
const statusDot = document.querySelector<HTMLSpanElement>("#status-dot")!;
const statusText = document.querySelector<HTMLSpanElement>("#status-text")!;
const levelFill = document.querySelector<HTMLDivElement>("#level-fill")!;
const messagesEl = document.querySelector<HTMLDivElement>("#transcript")!;
const emptyState = document.querySelector<HTMLDivElement>("#empty-state")!;
const emptyHint = document.querySelector<HTMLParagraphElement>("#empty-hint")!;
const partialEl = document.querySelector<HTMLParagraphElement>("#partial")!;
const exportTxtBtn = document.querySelector<HTMLButtonElement>("#export-txt")!;
const exportSrtBtn = document.querySelector<HTMLButtonElement>("#export-srt")!;
const exportPathEl = document.querySelector<HTMLSpanElement>("#export-path")!;
const modelProgressEl = document.querySelector<HTMLElement>("#model-progress")!;
const modelProgressFill = document.querySelector<HTMLDivElement>("#model-progress-fill")!;
const modelProgressText = document.querySelector<HTMLSpanElement>("#model-progress-text")!;
const toastEl = document.querySelector<HTMLDivElement>("#toast")!;
// "New transcription" button is optional (not rendered in the current layout).
const newSessionBtn = document.querySelector<HTMLButtonElement>("#new-session-btn");
const historyList = document.querySelector<HTMLDivElement>(".history-list")!;
const historyCount = document.querySelector<HTMLSpanElement>("#history-count")!;
const historyEmpty = document.querySelector<HTMLDivElement>("#history-empty")!;
const chatTitle = document.querySelector<HTMLHeadingElement>(".new-title")!;

type SessionState = "idle" | "recording" | "paused";
let sessionState: SessionState = "idle";
let timerInterval: number | undefined;
let elapsedSec = 0;
let toastTimer: number | undefined;
/// Stored session shown in the chat; the continue-recording target. Null when
/// the chat is on the live/new feed.
let viewedSessionId: number | null = null;

function setStatus(
  state: "idle" | "recording" | "paused" | "busy" | "error",
  text: string,
): void {
  statusDot.className = `status-dot ${state}`;
  statusText.textContent = text;
}

function formatTime(totalSec: number): string {
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function startTimer(): void {
  elapsedSec = 0;
  timerEl.textContent = formatTime(0);
  timerInterval = window.setInterval(() => {
    elapsedSec += 1;
    timerEl.textContent = formatTime(elapsedSec);
  }, 1000);
}

function stopTimer(): void {
  if (timerInterval !== undefined) {
    window.clearInterval(timerInterval);
    timerInterval = undefined;
  }
}

/// Reflect the session state across the UI. Used by both the local transport
/// and the tray menu's `recording-state` events.
function setSessionState(state: SessionState): void {
  sessionState = state;

  const active = state !== "idle";
  recordBtn.classList.toggle("recording", state === "recording");
  recordBtn.classList.toggle("paused", state === "paused");
  recordBtn.setAttribute(
    "aria-label",
    state === "idle"
      ? "Start recording"
      : state === "recording"
        ? "Pause recording"
        : "Resume recording",
  );
  recordIconPlay.toggleAttribute("hidden", state === "recording");
  recordIconPause.toggleAttribute("hidden", state !== "recording");
  stopBtn.hidden = !active;
  modelSelect.disabled = active;
  partialEl.classList.toggle("recording", state === "recording");

  if (state === "recording") {
    startTimer();
    setStatus("recording", "Recording");
  } else if (state === "paused") {
    stopTimer();
    setStatus("paused", "Paused");
  } else {
    stopTimer();
    setStatus("idle", "Idle");
  }

  emptyHint.textContent =
    state === "idle"
      ? "Start recording to transcribe your meeting."
      : state === "recording"
        ? "Listening…"
        : "Paused — press Resume to continue.";
}

/// Friendly, actionable message for known failure modes (S13 permission UX).
function describeError(err: unknown): string {
  const raw = String(err);
  const lower = raw.toLowerCase();
  if (lower.includes("screen recording") || lower.includes("screenrecording")) {
    return "Screen Recording permission needed: System Settings → Privacy & Security → Screen Recording, enable Roofle, then restart.";
  }
  if (lower.includes("microphone") || lower.includes("mic")) {
    return "Microphone permission needed: System Settings → Privacy & Security → Microphone, enable Roofle.";
  }
  if (lower.includes("model") && lower.includes("download")) {
    return `Model issue: ${raw}`;
  }
  return raw;
}

/// Whether the reader is at (or hasn't overflowed past) the bottom of the feed.
function isNearBottom(): boolean {
  return (
    messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight < 32
  );
}

/// Follow the latest message: only scroll when the feed is full and the reader
/// is at the bottom, so scrolling up to read earlier text isn't interrupted.
function scrollToLatest(): void {
  if (isNearBottom()) {
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }
}

/// Render each finalized utterance as its own chat message bubble.
function appendFinal(text: string): void {
  const p = document.createElement("p");
  p.className = "final";
  p.textContent = text;
  messagesEl.appendChild(p);
  partialEl.textContent = "";
  emptyState.hidden = true;
  exportTxtBtn.disabled = false;
  exportSrtBtn.disabled = false;
  scrollToLatest();
}

function showToast(msg: string): void {
  toastEl.textContent = msg;
  toastEl.classList.add("show");
  if (toastTimer !== undefined) {
    window.clearTimeout(toastTimer);
  }
  toastTimer = window.setTimeout(() => {
    toastEl.classList.remove("show");
  }, 2200);
}

/// The single transport button: Start → Pause → Resume → Pause….
/// Clear the current transcript from the UI (client-side only for now; session
/// persistence to the left "Transcriptions" panel comes with the backend later).
function clearTranscriptUI(): void {
  messagesEl.querySelectorAll(".final").forEach((el) => el.remove());
  partialEl.textContent = "";
  emptyState.hidden = false;
  exportTxtBtn.disabled = true;
  exportSrtBtn.disabled = true;
  exportPathEl.textContent = "";
}

/// Render the stored sessions in the left history list: a single-line
/// first-words preview per session. The open session is highlighted.
async function loadHistory(): Promise<void> {
  const sessions = await listSessions();
  historyList.querySelectorAll(".history-item").forEach((el) => el.remove());
  historyCount.textContent = String(sessions.length);

  historyEmpty.hidden = sessions.length !== 0;
  if (sessions.length === 0) {
    return;
  }

  for (const s of sessions) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "history-item";
    item.dataset.id = String(s.id);
    if (s.id === viewedSessionId) {
      item.classList.add("is-active");
    }

    const preview = document.createElement("span");
    preview.className = "history-preview";
    preview.textContent = s.preview || "(no text)";

    item.appendChild(preview);
    item.addEventListener("click", () => openSession(s.id));
    historyList.appendChild(item);
  }
}

/// Show a stored session's paragraph transcript in the chat pane.
async function openSession(id: number): Promise<void> {
  try {
    const detail = await getSession(id);
    if (!detail) return;

    viewedSessionId = id;
    chatTitle.textContent = "Transcription";
    messagesEl.querySelectorAll(".final").forEach((el) => el.remove());
    partialEl.textContent = "";
    emptyState.hidden = true;

    for (const para of detail.transcript.split("\n\n")) {
      if (!para.trim()) continue;
      const p = document.createElement("p");
      p.className = "final";
      p.textContent = para;
      messagesEl.appendChild(p);
    }

    // Export targets the live buffer, not a stored session.
    exportTxtBtn.disabled = true;
    exportSrtBtn.disabled = true;
    exportPathEl.textContent = "";
    scrollToLatest();

    // Re-render the list so the open session is highlighted as active.
    await loadHistory();
  } catch (err) {
    setStatus("error", String(err));
  }
}

/// "New transcription": stop any active session, then reset the transcript.
async function newSession(): Promise<void> {
  if (sessionState !== "idle") {
    await stop();
  }
  clearTranscriptUI();
  showToast("New transcription");
}

async function onMainControl(): Promise<void> {
  if (sessionState === "idle") {
    await start();
  } else if (sessionState === "recording") {
    await pause();
  } else {
    await resume();
  }
}

async function start(): Promise<void> {
  try {
    // Continuing a selected stored session appends to it; otherwise a fresh
    // recording is started. `sessionId: null` = new session.
    const continueId = viewedSessionId;
    await invoke("start_recording", { source: "both", sessionId: continueId });
    // On continue the selected session stays highlighted; a fresh recording
    // had no selection, so nothing to clear.
    chatTitle.textContent = continueId ? "Transcription" : "New Transcription";
    if (continueId === null) {
      clearTranscriptUI();
    }
    setSessionState("recording");
  } catch (err) {
    setStatus("error", describeError(err));
  }
}

async function pause(): Promise<void> {
  try {
    await invoke("pause_recording");
    setSessionState("paused");
  } catch (err) {
    setStatus("error", describeError(err));
  }
}

async function resume(): Promise<void> {
  try {
    await invoke("resume_recording");
    setSessionState("recording");
  } catch (err) {
    setStatus("error", describeError(err));
  }
}

async function stop(): Promise<void> {
  try {
    await invoke("stop_recording");
  } finally {
    setSessionState("idle");
  }
}

async function loadModels(): Promise<void> {
  const models = await invoke<ModelInfo[]>("list_models");
  modelSelect.innerHTML = "";
  for (const model of models) {
    const option = document.createElement("option");
    option.value = model.id;
    option.textContent = model.present
      ? model.label
      : `${model.label} (download on first run)`;
    modelSelect.appendChild(option);
  }
  modelSelect.value = await invoke<string>("selected_model");
}

async function onModelChange(): Promise<void> {
  try {
    await invoke("select_model", { id: modelSelect.value });
  } catch (err) {
    setStatus("error", String(err));
  }
}

async function exportTranscript(format: "txt" | "srt"): Promise<void> {
  try {
    const path = await invoke<string>("export_transcript", { format });
    exportPathEl.textContent = path;
    showToast(`Exported .${format}`);
  } catch (err) {
    setStatus("error", String(err));
  }
}

async function init(): Promise<void> {
  recordBtn.addEventListener("click", onMainControl);
  stopBtn.addEventListener("click", stop);
  newSessionBtn?.addEventListener("click", newSession);
  modelSelect.addEventListener("change", onModelChange);
  exportTxtBtn.addEventListener("click", () => exportTranscript("txt"));
  exportSrtBtn.addEventListener("click", () => exportTranscript("srt"));

  await loadModels();
  await loadHistory();

  await listen<Segment>("transcript-update", (event) => {
    const { kind, text } = event.payload;
    if (kind === "Final") {
      appendFinal(text);
    } else {
      partialEl.textContent = text;
    }
  });

  await listen<number>("level", (event) => {
    const pct = Math.min(100, Math.max(0, event.payload * 100));
    levelFill.style.width = `${pct}%`;
  });

  await listen<ModelProgress>("model-progress", (event) => {
    const { id, downloaded, total } = event.payload;
    if (total <= 0) return;
    const pct = Math.round((downloaded / total) * 100);
    setStatus("busy", "Preparing model…");
    modelProgressEl.hidden = false;
    modelProgressFill.style.width = `${pct}%`;
    modelProgressText.textContent = `Downloading ${id}… ${pct}%`;
    if (pct >= 100) {
      window.setTimeout(() => {
        modelProgressEl.hidden = true;
        modelProgressFill.style.width = "0%";
        if (sessionState === "idle") {
          setStatus("idle", "Idle");
        }
      }, 900);
    }
  });

  // S14: stay in sync with recordings started/stopped from the tray menu, and
  // refresh the history list (a session is opened/finalized on Start/Stop).
  // S14: stay in sync with recordings started/stopped from the tray menu, and
  // refresh the history list (a session is opened/finalized on Start/Stop).
  await listen<boolean>("recording-state", async (event) => {
    setSessionState(event.payload ? "recording" : "idle");
    await loadHistory();
  });

  // S14: transient tray status/errors (e.g. first-run model download failure).
  await listen<string>("app-status", (event) => {
    setStatus("error", event.payload);
  });
}

init();
