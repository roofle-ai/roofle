import { invoke } from "@tauri-apps/api/core";
import { session } from "../app/state";
import { dom } from "../lib/dom";
import { setStatus, showToast } from "../lib/status";
import { saveNotes } from "../transcripts";
import { blurNotes, getNotes, onNotesChange, scrollNotesToEnd, setNotes } from "./notes-editor";
import { syncWelcome } from "./welcome";

const AUTOSAVE_DELAY_MS = 800;
let saveTimer: number | undefined;

/** Replace the notepad's text with a session's saved notes; nothing is pending afterwards. */
export function setNotesText(text: string): void {
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  setNotes(text);
  syncWelcome();
}

/** Live transcript words if any, else "Notes / <date> / <time>". */
export function notesTitle(date: Date = new Date()): string {
  if (session.titleFromTranscript) return session.titleFromTranscript;
  const d = date.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
  const t = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Notes / ${d} / ${t}`;
}

/** Show when the meeting happened above the notes (e.g. "Thursday, 2 Oct 2026 · 14:05"). */
function renderNotesMeta(date: Date): void {
  const d = date.toLocaleDateString([], { weekday: "long", day: "numeric", month: "short", year: "numeric" });
  const t = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  dom.notesMeta.textContent = `${d} · ${t}`;
  dom.notesMeta.dateTime = date.toISOString();
}

/** Grey "12.4k tokens" at the right of the notes header; blank until the session has used any. */
export function refreshTokens(): void {
  const id = session.viewedId;
  const show = (text: string): void => {
    dom.notesTokens.textContent = text;
  };
  if (id === null) return show("");
  const compact = new Intl.NumberFormat([], { notation: "compact", maximumFractionDigits: 1 });
  invoke<[number, number]>("session_usage", { id })
    .then(([i, o]) => show(session.viewedId === id && i + o > 0 ? `${compact.format(i + o)} tokens` : ""))
    .catch(() => show(""));
}

export function refreshNotesTitle(date?: Date): void {
  refreshTokens();
  dom.chatTitle.textContent = notesTitle(date);
  // Continuing a saved session keeps its original start time.
  if (date || session.viewedId === null || !dom.notesMeta.dateTime) renderNotesMeta(date ?? new Date());
}

export function clearNotes(): void {
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  setNotes("");
  syncWelcome();
  session.titleFromTranscript = undefined;
  renderNotesMeta(new Date());
}

/** Every edit is saved shortly after the user pauses typing. */
export function scheduleNotesSave(): void {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined;
    void saveNotesNow();
  }, AUTOSAVE_DELAY_MS);
}

/** Persist the current notes to the viewed session; true when saved. */
export async function saveNotesNow(): Promise<boolean> {
  if (session.viewedId === null) return false;
  try {
    await saveNotes(session.viewedId, getNotes());
    return true;
  } catch (err) {
    setStatus("error", String(err));
    return false;
  }
}

/** Replace the notepad with the agent's full notes document. */
export function applyGeneratedNotes(text: string): void {
  const notes = text.trim();
  if (!notes) return;
  // A finished session's notes are already saved by the backend; a live one autosaves.
  if (session.state === "idle") {
    setNotesText(notes);
  } else {
    setNotes(notes);
    scheduleNotesSave();
  }
  scrollNotesToEnd();
  syncWelcome();
}

export function initNotes(): void {
  onNotesChange(scheduleNotesSave);
  dom.notesInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") blurNotes();
  });
  // ⌘S saves right away, for the muscle memory.
  window.addEventListener("keydown", (e) => {
    if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "s") return;
    e.preventDefault();
    void saveNotesNow().then((ok) => ok && showToast("Notes saved"));
  });
  renderNotesMeta(new Date());
}
