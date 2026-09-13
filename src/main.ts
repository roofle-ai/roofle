// Roofle frontend — minimal meeting copilot UI.
//
// A single transport control (Start / Pause / Resume / Stop) drives ONE
// recording session that captures both the microphone and system audio mixed
// into a single transcript, rendered as plain essay paragraphs. The Rust shell
// exposes start/pause/resume/stop commands and emits `transcript-update`,
// `level`, `model-progress`, `recording-state` and `app-status` events.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

type SessionState = "idle" | "recording" | "paused";
let sessionState: SessionState = "idle";
let timerInterval: number | undefined;
let elapsedSec = 0;
let toastTimer: number | undefined;

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
    // One session captures both mic + system audio (backend `Both` source).
    await invoke("start_recording", { source: "both" });
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

  // S14: stay in sync with recordings started/stopped from the tray menu.
  await listen<boolean>("recording-state", (event) => {
    setSessionState(event.payload ? "recording" : "idle");
  });

  // S14: transient tray status/errors (e.g. first-run model download failure).
  await listen<string>("app-status", (event) => {
    setStatus("error", event.payload);
  });
}

init();
