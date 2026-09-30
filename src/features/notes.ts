import { session } from "../app/state";
import { dom } from "../lib/dom";
import { setStatus } from "../lib/status";
import { saveNotes } from "../transcripts";

const AUTOSAVE_DELAY_MS = 800;
let saveTimer: number | undefined;

export function scrollNotesToEnd(): void {
  dom.notesInput.scrollTop = dom.notesInput.scrollHeight;
}

/** Live transcript words if any, else "Notes / <date> / <time>". */
export function notesTitle(date: Date = new Date()): string {
  if (session.titleFromTranscript) return session.titleFromTranscript;
  const d = date.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
  const t = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Notes / ${d} / ${t}`;
}

export function refreshNotesTitle(date?: Date): void {
  dom.chatTitle.textContent = notesTitle(date);
}

export function clearNotes(): void {
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  dom.notesInput.value = "";
  session.titleFromTranscript = undefined;
}

export function scheduleNotesSave(): void {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined;
    void saveNotesNow();
  }, AUTOSAVE_DELAY_MS);
}

export async function saveNotesNow(): Promise<void> {
  if (session.viewedId === null) return;
  try {
    await saveNotes(session.viewedId, dom.notesInput.value);
  } catch (err) {
    setStatus("error", String(err));
  }
}

/** Replace the editor with the agent's full notes document and autosave it. */
export function applyGeneratedNotes(text: string): void {
  const notes = text.trim();
  if (!notes) return;
  dom.notesInput.value = notes;
  scrollNotesToEnd();
  // Programmatic writes don't fire `input`.
  scheduleNotesSave();
}

export function initNotes(): void {
  dom.notesInput.addEventListener("input", scheduleNotesSave);
}
