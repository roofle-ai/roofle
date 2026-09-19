// Roofle frontend — "Meeting Copilot Deck" UI.
//
// A single transport control (Start / Pause / Resume / Stop) drives ONE
// recording session that captures both the microphone and system audio mixed
// into a single transcript. The main panel is an editable "Notes" page: live
// transcription auto-appends as plain text (only when the caret is at the end),
// the user can edit/overwrite freely, and the final notes (seed/transcript +
// user edits) are persisted per session via `save_notes`.
//
// The right panel is the product USP: LIVE follow-up questions derived from the
// transcript. Until the Rust analysis pipeline ships a real endpoint, questions
// are produced by a lightweight client-side heuristic (see analyzeSegment),
// clearly marked below as a stand-in. The Rust shell exposes start/pause/
// resume/stop commands and emits `transcript-update`, `level`,
// `model-progress`, `recording-state` and `app-status` events.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  CircleCheck,
  CircleDot,
  CornerDownRight,
  createIcons,
  MessageSquareQuote,
  Moon,
  Plus,
  Settings,
  Sparkles,
  Sun,
  Trash,
} from "lucide";
import { loadStoredApiKey } from "./ai";
import { openSettings } from "./settings";
import { deleteSession, getSession, listSessions, saveNotes } from "./transcripts";
import "./ui";

/// Payload of a `transcript-update` event (mirrors roofle_core::asr::Segment).
interface Segment {
  kind: "Partial" | "Final";
  text: string;
}

/// Payload of a `model-progress` event.
interface ModelProgress {
  id: string;
  downloaded: number;
  total: number;
}

/// Payload of a `notes-update` event (AI-generated meeting notes).
interface NotesUpdate {
  seq: number;
  text: string;
}

/// Payload of a `challenge-update` event (AI-generated follow-up questions).
interface ChallengeUpdate {
  seq: number;
  text: string;
}

/// Category of a follow-up question.
type QuestionType = "Clarification" | "Action" | "Risk" | "Decision" | "Open";

/// A single follow-up question card (the right-panel USP).
interface FollowUp {
  id: number;
  type: QuestionType;
  text: string;
  timestamp: number; // seconds into the session
  blockIndex: number; // id of the source transcript block
  resolved: boolean;
}

// ---- DOM refs --------------------------------------------------------------
const recordBtn = document.querySelector<HTMLButtonElement>("#record-btn")!;
const recordIconPlay = document.querySelector<SVGElement>("#record-icon-play")!;
const recordIconPause = document.querySelector<SVGElement>("#record-icon-pause")!;
const stopBtn = document.querySelector<HTMLButtonElement>("#stop-btn")!;
const timerEl = document.querySelector<HTMLSpanElement>("#timer")!;
const sessionPill = document.querySelector<HTMLSpanElement>("#session-pill")!;
const statusDot = document.querySelector<HTMLSpanElement>("#status-dot")!;
const statusText = document.querySelector<HTMLSpanElement>("#status-text")!;
const levelFill = document.querySelector<HTMLDivElement>("#level-fill")!;
const notesInput = document.querySelector<HTMLTextAreaElement>("#notes-input")!;
const modelProgressEl = document.querySelector<HTMLElement>("#model-progress")!;
const modelProgressFill = document.querySelector<HTMLDivElement>("#model-progress-fill")!;
const modelProgressText = document.querySelector<HTMLSpanElement>("#model-progress-text")!;
const toastEl = document.querySelector<HTMLDivElement>("#toast")!;
const floatingControl = document.querySelector<HTMLDivElement>("#floating-control")!;
const transcriptPanel = document.querySelector<HTMLElement>("#transcript-panel")!;
const newSessionBtn = document.querySelector<HTMLButtonElement>("#new-session-btn");
const settingsBtn = document.querySelector<HTMLButtonElement>("#settings-btn");
const historyList = document.querySelector<HTMLDivElement>(".history-list")!;
const historyCount = document.querySelector<HTMLSpanElement>("#history-count")!;
const historyEmpty = document.querySelector<HTMLDivElement>("#history-empty")!;
const chatTitle = document.querySelector<HTMLHeadingElement>("#chat-title")!;
const themeToggle = document.querySelector<HTMLButtonElement>("#theme-toggle")!;
const workspace = document.querySelector<HTMLDivElement>(".workspace")!;
const questionsList = document.querySelector<HTMLDivElement>("#questions-list")!;
const questionsEmpty = document.querySelector<HTMLDivElement>("#questions-empty")!;
const questionsEmptyHint = document.querySelector<HTMLParagraphElement>("#questions-empty-hint")!;
const qLive = document.querySelector<HTMLSpanElement>("#q-live")!;
const summaryCard = document.querySelector<HTMLDivElement>("#summary-card")!;
const summaryStats = document.querySelector<HTMLDivElement>("#summary-stats")!;
const summaryNote = document.querySelector<HTMLParagraphElement>("#summary-note")!;

// ---- Icons ----------------------------------------------------------------
const ICONS = {
  Plus,
  Settings,
  Moon,
  Sun,
  MessageSquareQuote,
  Sparkles,
  CircleDot,
  CircleCheck,
  CornerDownRight,
  Trash,
};

function refreshIcons(): void {
  createIcons({ icons: ICONS });
}

// Safe storage access — never let a sandboxed/disabled localStorage break init.
function storeGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storeSet(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore */
  }
}

// ---- State -----------------------------------------------------------------
type SessionState = "idle" | "recording" | "paused";
let sessionState: SessionState = "idle";
let timerInterval: number | undefined;
let elapsedSec = 0;
let toastTimer: number | undefined;
let viewedSessionId: number | null = null;

/// First words of the live transcription, used for the main-panel title once
/// speech arrives. Cleared whenever the viewed session changes.
let titleFromTranscript: string | undefined;

let pendingAppend: string[] = [];
let notesSaveTimer: number | undefined;

let questions: FollowUp[] = [];
let nextQuestionId = 0;
let analysisMode = false;

// ---- Theme (light / dark, defaults to system) ------------------------------
function applyTheme(theme: "light" | "dark"): void {
  document.documentElement.setAttribute("data-theme", theme);
  themeToggle.innerHTML = `<i data-lucide="${theme === "dark" ? "sun" : "moon"}" aria-hidden="true"></i>`;
  themeToggle.setAttribute(
    "aria-label",
    theme === "dark" ? "Switch to light theme" : "Switch to dark theme",
  );
  refreshIcons();
  storeSet("roofle-theme", theme);
}

function initTheme(): void {
  const saved = storeGet("roofle-theme") as "light" | "dark" | null;
  const theme: "light" | "dark" =
    saved ?? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  applyTheme(theme);
}

// ---- Status / timer ---------------------------------------------------------
function setStatus(
  state: "idle" | "recording" | "paused" | "busy" | "error",
  text: string,
): void {
  statusDot.className = `status-dot ${state}`;
  statusText.textContent = text;
  const pillState = state === "busy" ? "paused" : state;
  sessionPill.className = `session-pill ${pillState}`;
}

function formatTime(totalSec: number): string {
  // Stored segment start_secs can be a float; floor to whole seconds so
  // timestamps render as MM:SS (never "01:8.473834417000006").
  const total = Math.floor(totalSec);
  const m = Math.floor(total / 60);
  const s = total % 60;
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

// ---- Notes rendering -------------------------------------------------------
/// Whether the caret sits at the very end of the notes — the only safe time to
/// auto-append live text without disturbing the user's edits.
function isCaretAtEnd(): boolean {
  return (
    notesInput.selectionStart === notesInput.selectionEnd &&
    notesInput.selectionStart === notesInput.value.length
  );
}

/// Keep the notes pinned to the bottom (follows live transcription).
function notesScrollToEnd(): void {
  notesInput.scrollTop = notesInput.scrollHeight;
}

/// Number of leading words to show in the main-panel title (mirrors the
/// history-list preview length).
const TITLE_WORDS = 10;

/// First `max` whitespace-separated words of `text`.
function previewWords(text: string, max: number = TITLE_WORDS): string {
  return text.split(/\s+/).filter(Boolean).slice(0, max).join(" ");
}

/// The main-panel title. Once live transcription arrives it shows the first few
/// words of the transcript (same signal as the history preview); before any
/// speech it falls back to "Notes / <date> / <time>".
function notesTitle(date: Date = new Date()): string {
  if (titleFromTranscript) return titleFromTranscript;
  const d = date.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
  const t = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Notes / ${d} / ${t}`;
}

/// Append any buffered final text once the caret is back at the end.
function flushPending(): void {
  if (pendingAppend.length === 0) return;
  if (!isCaretAtEnd()) return;
  const tail = pendingAppend.join("\n");
  pendingAppend = [];
  notesInput.value = notesInput.value ? notesInput.value + "\n" + tail : tail;
  notesScrollToEnd();
}

// ---- Follow-up questions (agent-driven) ------------------------------------
// Follow-up questions come from the live challenge agent via the
// `challenge-update` event — the agent returns the full question list as plain
// text (one question per line). We ingest it into the right panel, deduping
// against what's already shown so the feed grows rather than repeats.
function ingestChallenges(text: string, timestamp: number): void {
  const lines = text
    .split(/\r?\n/)
    .map((l) => l.trim().replace(/^[-*•\d.)\s]+/, ""))
    .filter((l) => l.length > 0);
  if (lines.length === 0) return;

  const norm = (s: string) => s.toLowerCase().replace(/\s+/g, " ").replace(/[?.!]+$/, "").trim();
  for (const line of lines) {
    const key = norm(line);
    if (questions.some((q) => norm(q.text) === key)) continue;
    const q: FollowUp = {
      id: nextQuestionId++,
      type: "Open",
      text: line,
      timestamp,
      blockIndex: 0,
      resolved: false,
    };
    questions.push(q);
    renderQuestion(q);
  }
}

function renderQuestion(q: FollowUp): void {
  questionsEmpty.hidden = true;
  const card = document.createElement("article");
  card.className = "qcard";
  card.dataset.qid = String(q.id);

  const chip = document.createElement("span");
  chip.className = `chip ${q.type}`;
  chip.textContent = q.type;
  const ts = document.createElement("span");
  ts.className = "timestamp";
  ts.textContent = formatTime(q.timestamp);
  const top = document.createElement("div");
  top.className = "qcard-top";
  top.appendChild(chip);
  top.appendChild(ts);

  const text = document.createElement("p");
  text.className = "qtext";
  text.textContent = q.text;

  const stateLabel = document.createElement("span");
  stateLabel.className = "q-state";
  stateLabel.textContent = q.resolved ? "Resolved" : "Unanswered";
  const metaLeft = document.createElement("div");
  metaLeft.className = "qmeta-left";
  metaLeft.appendChild(stateLabel);

  const jumpBtn = document.createElement("button");
  jumpBtn.type = "button";
  jumpBtn.className = "qaction jump";
  jumpBtn.setAttribute("aria-label", "Jump to context in transcript");
  jumpBtn.innerHTML = `<i data-lucide="corner-down-right" aria-hidden="true"></i>`;
  jumpBtn.addEventListener("click", () => jumpToBlock(q.blockIndex));

  const resolveBtn = document.createElement("button");
  resolveBtn.type = "button";
  resolveBtn.className = "qaction resolve";
  resolveBtn.setAttribute("aria-label", q.resolved ? "Mark unanswered" : "Mark resolved");
  resolveBtn.innerHTML = `<i data-lucide="${q.resolved ? "circle-check" : "circle-dot"}" aria-hidden="true"></i>`;
  resolveBtn.addEventListener("click", () => {
    q.resolved = !q.resolved;
    card.classList.toggle("resolved", q.resolved);
    stateLabel.textContent = q.resolved ? "Resolved" : "Unanswered";
    resolveBtn.setAttribute("aria-label", q.resolved ? "Mark unanswered" : "Mark resolved");
    resolveBtn.innerHTML = `<i data-lucide="${q.resolved ? "circle-check" : "circle-dot"}" aria-hidden="true"></i>`;
    refreshIcons();
    if (analysisMode) renderSummary();
  });

  const actions = document.createElement("div");
  actions.className = "qactions";
  actions.appendChild(jumpBtn);
  actions.appendChild(resolveBtn);

  const meta = document.createElement("div");
  meta.className = "qmeta";
  meta.appendChild(metaLeft);
  meta.appendChild(actions);

  card.appendChild(top);
  card.appendChild(text);
  card.appendChild(meta);

  // Newest-first feed.
  questionsList.prepend(card);
  refreshIcons();
  updateLiveBadge();
}

function jumpToBlock(_id: number): void {
  // The notes are a single editable document, so "jump to context" focuses the
  // editor and scrolls to the live append point.
  notesInput.focus();
  notesScrollToEnd();
}

function resetQuestions(): void {
  questions = [];
  questionsList.querySelectorAll(".qcard").forEach((el) => el.remove());
  summaryCard.hidden = true;
  questionsEmpty.hidden = false;
  questionsEmptyHint.textContent =
    "Roofle is listening… follow-up questions will appear here as the conversation unfolds.";
  qLive.className = "live-badge";
  qLive.textContent = "";
}

function updateLiveBadge(): void {
  if (analysisMode) {
    const unresolved = questions.filter((q) => !q.resolved).length;
    qLive.className = "live-badge show analysis";
    qLive.textContent = questions.length === 0 ? "" : unresolved === 0 ? "All resolved" : `${unresolved} open`;
    return;
  }
  qLive.className = "live-badge show live";
  qLive.textContent = questions.length === 0 ? "" : `${questions.length} live`;
}

function renderSummary(): void {
  if (questions.length === 0) {
    summaryCard.hidden = true;
    return;
  }
  summaryCard.hidden = false;
  const counts: Record<string, number> = { Clarification: 0, Action: 0, Risk: 0, Decision: 0, Open: 0 };
  questions.forEach((q) => (counts[q.type] = (counts[q.type] || 0) + 1));
  const unresolved = questions.filter((q) => !q.resolved).length;

  summaryStats.innerHTML = "";
  const add = (n: number, label: string) => {
    const stat = document.createElement("div");
    stat.className = "stat";
    const b = document.createElement("b");
    b.textContent = String(n);
    const s = document.createElement("span");
    s.textContent = label;
    stat.appendChild(b);
    stat.appendChild(s);
    summaryStats.appendChild(stat);
  };
  add(questions.length, "questions");
  add(unresolved, "open");
  (Object.keys(counts) as QuestionType[]).forEach((type) => {
    if (counts[type]) add(counts[type], type.toLowerCase());
  });
  summaryNote.textContent = unresolved
    ? `${unresolved} follow-up ${unresolved === 1 ? "question remains" : "questions remain"} open for the team.`
    : "All follow-up questions resolved. Nice work.";
  updateLiveBadge();
}

function setAnalysisMode(on: boolean): void {
  analysisMode = on;
  if (on) {
    renderSummary();
  } else {
    summaryCard.hidden = true;
    updateLiveBadge();
  }
}

// ---- Transport --------------------------------------------------------------
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

  questionsEmptyHint.textContent =
    state === "recording"
      ? "Roofle is listening… follow-up questions will appear here as the conversation unfolds."
      : state === "paused"
        ? "Paused — follow-up questions will resume when you resume recording."
        : "Roofle is listening… follow-up questions will appear here as the conversation unfolds.";
}

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

/// Make the floating transport control freely draggable anywhere in the main
/// panel. It starts centered over the transcript and follows the pointer.
function initFloatingControl(): void {
  const panel = transcriptPanel;

  const center = () => {
    const pr = panel.getBoundingClientRect();
    const w = floatingControl.offsetWidth;
    const h = floatingControl.offsetHeight;
    floatingControl.style.left = `${Math.max(0, (pr.width - w) / 2)}px`;
    floatingControl.style.top = `${Math.max(0, (pr.height - h) / 2)}px`;
  };
  center();
  window.addEventListener("resize", center);

  let drag: { dx: number; dy: number } | null = null;
  floatingControl.addEventListener("pointerdown", (e) => {
    // Don't start dragging when pressing a control button inside the pill.
    if ((e.target as HTMLElement).closest("button")) return;
    const r = floatingControl.getBoundingClientRect();
    drag = { dx: e.clientX - r.left, dy: e.clientY - r.top };
    floatingControl.classList.add("dragging");
    floatingControl.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  });
  window.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const pr = panel.getBoundingClientRect();
    const x = e.clientX - pr.left - drag.dx;
    const y = e.clientY - pr.top - drag.dy;
    const maxX = Math.max(0, pr.width - floatingControl.offsetWidth);
    const maxY = Math.max(0, pr.height - floatingControl.offsetHeight);
    floatingControl.style.left = `${Math.max(0, Math.min(x, maxX))}px`;
    floatingControl.style.top = `${Math.max(0, Math.min(y, maxY))}px`;
  });
  const stop = () => {
    if (!drag) return;
    drag = null;
    floatingControl.classList.remove("dragging");
  };
  window.addEventListener("pointerup", stop);
  window.addEventListener("pointercancel", stop);
}

function clearTranscriptUI(): void {
  if (notesSaveTimer !== undefined) {
    window.clearTimeout(notesSaveTimer);
    notesSaveTimer = undefined;
  }
  pendingAppend = [];
  notesInput.value = "";
  titleFromTranscript = undefined;
  resetQuestions();
  setAnalysisMode(false);
}

// ---- Notes persistence ------------------------------------------------------
/// Resolve the active session id for autosave without changing the start
/// command: when a fresh recording starts, the engine creates the session
/// backend-side without returning its id, so we adopt the newest recording
/// session from the list. For a continued session the id is already known.
async function resolveActiveSession(): Promise<void> {
  if (viewedSessionId !== null) return;
  try {
    const sessions = await listSessions();
    const recording = sessions.find((s) => s.status === "recording");
    const target = recording ?? sessions[0];
    if (target) viewedSessionId = target.id;
  } catch {
    // Non-fatal: autosave simply waits until a session id is known.
  }
}

/// Debounced autosave of the editable notes while a session is viewed.
function scheduleNotesSave(): void {
  if (notesSaveTimer !== undefined) window.clearTimeout(notesSaveTimer);
  notesSaveTimer = window.setTimeout(() => {
    notesSaveTimer = undefined;
    void finalSaveNotes();
  }, 800);
}

/// Persist the current notes editor content (the final notes = seed/transcript
/// + user edits) to the currently viewed session.
async function finalSaveNotes(): Promise<void> {
  if (viewedSessionId === null) return;
  try {
    await saveNotes(viewedSessionId, notesInput.value);
  } catch (err) {
    setStatus("error", String(err));
  }
}

// ---- History ----------------------------------------------------------------
function formatDuration(secs: number): string {
  if (secs <= 0) return "—";
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (m >= 60) return `${Math.floor(m / 60)}h ${m % 60}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

function formatWhen(iso: string): string {
  const d = new Date(iso);
  const diffDays = Math.floor((Date.now() - d.getTime()) / 86400000);
  if (diffDays <= 0) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (diffDays === 1) return "Yesterday";
  if (diffDays < 7) return d.toLocaleDateString([], { weekday: "short" });
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}

async function loadHistory(): Promise<void> {
  const sessions = await listSessions();
  historyList.querySelectorAll(".history-item").forEach((el) => el.remove());
  historyCount.textContent = String(sessions.length);
  historyEmpty.hidden = sessions.length !== 0;
  if (sessions.length === 0) return;

  for (const s of sessions) {
    const item = document.createElement("div");
    item.className = "history-item";
    item.dataset.id = String(s.id);
    if (s.id === viewedSessionId) item.classList.add("is-active");
    if (s.status === "recording") item.classList.add("is-recording");

    const title = document.createElement("span");
    title.className = "history-title";
    title.textContent = s.title || s.preview || "(untitled)";

    const meta = document.createElement("span");
    meta.className = "history-meta";
    meta.textContent = `${formatDuration(s.duration_secs)} · ${formatWhen(s.started_at)}`;

    item.appendChild(title);
    item.appendChild(meta);

    item.tabIndex = 0;
    item.setAttribute("role", "button");
    item.setAttribute("aria-label", `Open session ${title.textContent}`);
    item.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).closest(".history-delete")) return;
      void openSession(s.id);
    });
    item.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        void openSession(s.id);
      }
    });

    const del = document.createElement("button");
    del.type = "button";
    del.className = "history-delete";
    del.setAttribute("aria-label", "Delete session");
    del.title = "Delete session";
    del.innerHTML = `<i data-lucide="trash" aria-hidden="true"></i>`;
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      void deleteSession(s.id)
        .then(() => {
          if (viewedSessionId === s.id) {
            viewedSessionId = null;
            chatTitle.textContent = notesTitle();
            clearTranscriptUI();
          }
          void loadHistory();
        })
        .catch((err) => setStatus("error", String(err)));
    });
    item.appendChild(del);
    historyList.appendChild(item);
  }
  refreshIcons();
}

async function openSession(id: number): Promise<void> {
  try {
    const detail = await getSession(id);
    if (!detail) return;

    viewedSessionId = id;

    // Clear the live notes + questions, then load the stored notes document.
    clearTranscriptUI();
    // Title: for a session with existing transcription, show its first words
    // (same as the history preview); otherwise fall back to the placeholder.
    titleFromTranscript = detail.transcript?.trim()
      ? previewWords(detail.transcript)
      : undefined;
    chatTitle.textContent = notesTitle(new Date(detail.session.started_at));
    notesInput.value = detail.notes ?? "";

    // Follow-up questions are produced live by the challenge agent and arrive
    // via `challenge-update`; re-opening a session shows whatever was captured
    // for it during the live run.
    setAnalysisMode(true);
    await loadHistory();
  } catch (err) {
    setStatus("error", String(err));
  }
}

// ---- Actions ----------------------------------------------------------------
async function newSession(): Promise<void> {
  if (sessionState !== "idle") {
    await stop();
  }
  viewedSessionId = null;
  chatTitle.textContent = notesTitle();
  clearTranscriptUI();
  showToast("New Meeting Notes");
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
    const continueId = viewedSessionId;
    if (continueId === null) {
      chatTitle.textContent = notesTitle();
      clearTranscriptUI();
    }
    resetQuestions();
    setAnalysisMode(false);
    await invoke("start_recording", {
      source: "both",
      sessionId: continueId,
      apiKey: loadStoredApiKey(),
    });
    if (continueId === null) {
      await resolveActiveSession();
    }
    if (continueId) chatTitle.textContent = notesTitle();
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
    // Capture any buffered final words, then persist the final notes document
    // (seed/transcript + any user edits) to the current session.
    flushPending();
    await finalSaveNotes();
    setAnalysisMode(true);
    await loadHistory();
  }
}

// ---- Panel resize -------------------------------------------------------------
function restoreWidths(): void {
  const hw = storeGet("roofle-history-w");
  if (hw) workspace.style.setProperty("--history-w", `${hw}px`);
  const qw = storeGet("roofle-questions-w");
  if (qw) workspace.style.setProperty("--questions-w", `${qw}px`);
}

function setupResizer(handle: HTMLElement): void {
  const side = handle.dataset.resizer as "history" | "questions";
  const cssVar = side === "history" ? "--history-w" : "--questions-w";
  const storageKey = `roofle-${side}-w`;
  const min = 180;
  const max = () => workspace.getBoundingClientRect().width * 0.5;
  let dragging = false;

  handle.addEventListener("pointerdown", (e) => {
    dragging = true;
    handle.classList.add("dragging");
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    handle.setPointerCapture?.(e.pointerId);
  });

  window.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const rect = workspace.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const w = side === "history" ? x : rect.width - x;
    workspace.style.setProperty(cssVar, `${Math.max(min, Math.min(w, max()))}px`);
  });

  const finish = () => {
    if (!dragging) return;
    dragging = false;
    handle.classList.remove("dragging");
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    const w = parseFloat(workspace.style.getPropertyValue(cssVar)) || 0;
    if (w) storeSet(storageKey, String(Math.round(w)));
  };
  window.addEventListener("pointerup", finish);
  window.addEventListener("pointercancel", finish);

  handle.addEventListener("keydown", (e) => {
    const step = 20;
    const cur = parseFloat(workspace.style.getPropertyValue(cssVar)) || (side === "history" ? 240 : 340);
    let w = cur;
    if (e.key === "ArrowLeft") w = side === "history" ? cur - step : cur + step;
    else if (e.key === "ArrowRight") w = side === "history" ? cur + step : cur - step;
    else return;
    e.preventDefault();
    const clamped = Math.max(min, Math.min(w, max()));
    workspace.style.setProperty(cssVar, `${clamped}px`);
    storeSet(storageKey, String(clamped));
  });
}

// ---- Keyboard shortcuts ------------------------------------------------------
function setupShortcuts(): void {
  window.addEventListener("keydown", (e) => {
    if (!(e.metaKey || e.ctrlKey)) return;
    const k = e.key.toLowerCase();
    if (k === "r") {
      e.preventDefault();
      void onMainControl();
    } else if (k === " ") {
      e.preventDefault();
      if (sessionState === "recording") void pause();
      else if (sessionState === "paused") void resume();
    } else if (k === ".") {
      e.preventDefault();
      if (sessionState !== "idle") void stop();
    }
  });
}

// ---- Init --------------------------------------------------------------------
async function init(): Promise<void> {
  initTheme();
  refreshIcons();

  recordBtn.addEventListener("click", onMainControl);
  stopBtn.addEventListener("click", () => void stop());
  newSessionBtn?.addEventListener("click", () => void newSession());
  settingsBtn?.addEventListener("click", () => openSettings());
  themeToggle.addEventListener("click", () => {
    const cur = document.documentElement.getAttribute("data-theme") === "dark" ? "dark" : "light";
    applyTheme(cur === "dark" ? "light" : "dark");
  });
  restoreWidths();
  document.querySelectorAll<HTMLElement>(".resizer").forEach(setupResizer);
  initFloatingControl();
  setupShortcuts();

  notesInput.addEventListener("input", scheduleNotesSave);
  notesInput.addEventListener("focus", flushPending);
  notesInput.addEventListener("click", flushPending);
  notesInput.addEventListener("keyup", () => {
    if (isCaretAtEnd()) flushPending();
  });

  await loadHistory();

  await listen<Segment>("transcript-update", (event) => {
    const { kind, text } = event.payload;
    // The main panel is notes-only: never write raw transcription here. The
    // follow-up questions arrive via the separate `challenge-update` event.
    if (kind === "Final") {
      // Title: once speech arrives, show the first words of the transcription
      // (same signal as the history preview) instead of the blank placeholder.
      if (!titleFromTranscript && text.trim()) {
        titleFromTranscript = previewWords(text);
        chatTitle.textContent = titleFromTranscript;
      }
    }
  });

  // AI-generated follow-up questions, driven by the live challenge agent. The
  // agent returns the full accumulated question list (one per line), so we
  // ingest it into the right panel, deduping as we go.
  await listen<ChallengeUpdate>("challenge-update", (event) => {
    ingestChallenges(event.payload.text, elapsedSec);
  });

  // AI-generated meeting notes replace the raw transcript in the notes panel.
  // The backend agent merges each new paragraph into the running notes document
  // and returns the FULL updated doc, so we replace the editor content rather
  // than append a block.
  await listen<NotesUpdate>("notes-update", (event) => {
    const notes = event.payload.text.trim();
    if (!notes) return;
    notesInput.value = notes;
    notesScrollToEnd();
    // Programmatic value writes don't fire the `input` event, so autosave the
    // updated notes explicitly.
    scheduleNotesSave();
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

  // Stay in sync with recordings started/stopped from the tray menu.
  await listen<boolean>("recording-state", async (event) => {
    if (event.payload) {
      setSessionState("recording");
      await resolveActiveSession();
    } else {
      setSessionState("idle");
      setAnalysisMode(true);
    }
    await loadHistory();
  });

  await listen<string>("app-status", (event) => {
    setStatus("error", event.payload);
  });
}

void init();
