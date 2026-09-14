// Roofle frontend — "Meeting Copilot Deck" UI.
//
// A single transport control (Start / Pause / Resume / Stop) drives ONE
// recording session that captures both the microphone and system audio mixed
// into a single transcript, rendered as speaker-timestamped document blocks.
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
import { openSettings } from "./settings";
import { deleteSession, getSession, listSessions } from "./transcripts";
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

/// A rendered transcript block (speaker + timestamp + text).
interface Block {
  id: number;
  speaker: string;
  timestamp: string;
  text: string;
  el: HTMLElement;
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
const messagesEl = document.querySelector<HTMLDivElement>("#transcript")!;
const emptyState = document.querySelector<HTMLDivElement>("#empty-state")!;
const emptyHint = document.querySelector<HTMLParagraphElement>("#empty-hint")!;
const partialEl = document.querySelector<HTMLParagraphElement>("#partial")!;
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

let blocks: Block[] = [];
let nextBlockId = 0;
let currentSpeaker: "Speaker A" | "Speaker B" = "Speaker A";

let questions: FollowUp[] = [];
let nextQuestionId = 0;
let analysisMode = false;

const prefersReduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

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

// ---- Transcript rendering ---------------------------------------------------
/// Always follow the latest text: keep the transcript pinned to the bottom.
function scrollToLatest(): void {
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function appendBlock(text: string, timestamp: string): Block {
  // Speaker heuristic (placeholder until the backend provides diarization):
  // a question mark or a turn-taking interjection suggests the speaker changes.
  const t = text.trim();
  if (blocks.length > 0) {
    const last = blocks[blocks.length - 1];
    if (
      last.text.trimEnd().endsWith("?") ||
      /^(okay|alright|thanks|right|sure|yes|yeah|so,?)/i.test(t)
    ) {
      currentSpeaker = currentSpeaker === "Speaker A" ? "Speaker B" : "Speaker A";
    }
  }
  const blockId = nextBlockId++;
  const blockEl = document.createElement("div");
  blockEl.className = "tblock";
  blockEl.dataset.block = String(blockId);
  const head = document.createElement("div");
  head.className = "tblock-head";
  const speaker = document.createElement("span");
  speaker.className = "speaker";
  speaker.textContent = currentSpeaker;
  const ts = document.createElement("span");
  ts.className = "timestamp";
  ts.textContent = timestamp;
  head.appendChild(speaker);
  head.appendChild(ts);
  const p = document.createElement("p");
  p.className = "ttext";
  p.textContent = t;
  blockEl.appendChild(head);
  blockEl.appendChild(p);
  messagesEl.appendChild(blockEl);
  const block: Block = { id: blockId, speaker: currentSpeaker, timestamp, text: t, el: blockEl };
  blocks.push(block);
  return block;
}

/// Append a finalized utterance, then run it through the question heuristic.
function appendFinal(text: string): void {
  appendBlock(text, formatTime(elapsedSec));
  partialEl.textContent = "";
  emptyState.hidden = true;
  maybeGenerateQuestion(text, elapsedSec, blocks[blocks.length - 1].id);
  scrollToLatest();
}

// ---- Follow-up question heuristic (client-side stand-in) --------------------
function makeQuestion(
  type: QuestionType,
  text: string,
  timestamp: number,
  blockIndex: number,
): FollowUp | null {
  const norm = (s: string) => s.toLowerCase().replace(/\s+/g, " ").replace(/[?.!]+$/, "").trim();
  const key = norm(text);
  if (questions.some((q) => norm(q.text) === key)) return null;
  const q: FollowUp = { id: nextQuestionId++, type, text, timestamp, blockIndex, resolved: false };
  questions.push(q);
  renderQuestion(q);
  return q;
}

function extractSubject(text: string): string {
  const words = text.split(/\s+/).slice(0, 8);
  while (
    words.length &&
    /^(so|okay|ok|alright|well|um|uh|and|but|now|right|yeah|yes|also|anyway|first|basically)$/i.test(
      words[0],
    )
  ) {
    words.shift();
  }
  let s = words.join(" ").replace(/[.,;:!?]+$/, "").trim();
  if (s.length > 42) s = s.slice(0, 42).trimEnd() + "…";
  return s || "this item";
}

function maybeGenerateQuestion(text: string, timestamp: number, blockIndex: number): void {
  const t = text.trim();
  if (!t) return;

  // Explicit question in the meeting → surface it directly.
  if (t.endsWith("?")) {
    makeQuestion("Clarification", t, timestamp, blockIndex);
    return;
  }

  const lower = t.toLowerCase();
  let type: QuestionType | null = null;
  if (
    /(who|we|i)\s+(own|owns|need|needs|should|will|must|have to|has to|gonna)/.test(lower) ||
    /\b(follow[- ]?up|next step|to[- ]?do|owner|deadline|due|assign|by (mon|tue|wed|thu|fri|tomorrow|eod|eow|end of (day|week)))\b/.test(lower)
  ) {
    type = "Action";
  } else if (/\b(risk|blocker|concern|issue|problem|danger|worried|critical|at stake)\b/.test(lower)) {
    type = "Risk";
  } else if (
    /\b(decide|decision|confirm|finalize|agree|approve|sign[- ]?off|we should|let'?s go with|go with)\b/.test(lower)
  ) {
    type = "Decision";
  }
  if (!type) return;

  const subject = extractSubject(t);
  let question: string;
  if (type === "Action") question = `Who is owning "${subject}" and by when?`;
  else if (type === "Risk") question = `What is the plan to de-risk "${subject}"?`;
  else question = `Is the decision on "${subject}" confirmed?`;
  makeQuestion(type, question, timestamp, blockIndex);
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

function jumpToBlock(id: number): void {
  const block = blocks.find((b) => b.id === id);
  if (!block) return;
  blocks.forEach((b) => b.el.classList.remove("flash"));
  block.el.scrollIntoView({ behavior: prefersReduced ? "auto" : "smooth", block: "center" });
  block.el.classList.add("flash");
  window.setTimeout(() => block.el.classList.remove("flash"), 1800);
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
      ? "" // the centered record button is the CTA when idle
      : state === "recording"
        ? "Listening…"
        : "Paused — press Resume to continue.";

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
  blocks = [];
  nextBlockId = 0;
  currentSpeaker = "Speaker A";
  messagesEl.querySelectorAll(".tblock").forEach((el) => el.remove());
  partialEl.textContent = "";
  emptyState.hidden = false;
  resetQuestions();
  setAnalysisMode(false);
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
            chatTitle.textContent = "New Transcription";
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
    chatTitle.textContent = "Transcription";

    // Clear the live transcript + questions, then rebuild from the stored run.
    clearTranscriptUI();
    emptyState.hidden = true;
    blocks = [];
    nextBlockId = 0;
    currentSpeaker = "Speaker A";

    if (detail.segments && detail.segments.length) {
      for (const seg of detail.segments) {
        if (!seg.text.trim()) continue;
        const block = appendBlock(seg.text, formatTime(seg.start_secs));
        maybeGenerateQuestion(seg.text, seg.start_secs, block.id);
      }
    } else {
      for (const para of detail.transcript.split("\n\n")) {
        if (!para.trim()) continue;
        const block = appendBlock(para.trim(), "00:00");
        maybeGenerateQuestion(para.trim(), 0, block.id);
      }
    }

    scrollToLatest();
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
  chatTitle.textContent = "New Transcription";
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
    const continueId = viewedSessionId;
    if (continueId === null) {
      chatTitle.textContent = "New Transcription";
      clearTranscriptUI();
    }
    resetQuestions();
    setAnalysisMode(false);
    await invoke("start_recording", { source: "both", sessionId: continueId });
    if (continueId) chatTitle.textContent = "Transcription";
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

  // Stay in sync with recordings started/stopped from the tray menu.
  await listen<boolean>("recording-state", async (event) => {
    if (event.payload) {
      setSessionState("recording");
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
