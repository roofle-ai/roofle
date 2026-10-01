import { session } from "../app/state";
import { dom } from "../lib/dom";
import { setStatus, showToast } from "../lib/status";
import { saveNotes } from "../transcripts";

const AUTOSAVE_DELAY_MS = 800;
let saveTimer: number | undefined;
/** True when the edited notes differ from what was last persisted. */
let dirty = false;

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
  dirty = false;
  dom.notesInput.value = "";
  session.titleFromTranscript = undefined;
  updateNotesSaveVisibility();
}

export function scheduleNotesSave(): void {
  window.clearTimeout(saveTimer);
  // A finished session saves explicitly (button / ⌘S) once the user edits.
  if (session.state === "idle") {
    dirty = true;
    updateNotesSaveVisibility();
    return;
  }
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined;
    void saveNotesNow();
  }, AUTOSAVE_DELAY_MS);
}

/** Persist the current notes to the viewed session; true when saved. */
export async function saveNotesNow(): Promise<boolean> {
  if (session.viewedId === null) return false;
  try {
    await saveNotes(session.viewedId, dom.notesInput.value);
    dirty = false;
    updateNotesSaveVisibility();
    return true;
  } catch (err) {
    setStatus("error", String(err));
    return false;
  }
}

/** Explicit save (button / ⌘S): persist and confirm. */
async function explicitSave(): Promise<void> {
  if (await saveNotesNow()) showToast("Notes saved");
}

/** Show the Save control only when editing a finished session with unsaved edits. */
export function updateNotesSaveVisibility(): void {
  const editable = session.state === "idle" && session.viewedId !== null && dirty;
  dom.notesSave.hidden = !editable;
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
  dom.notesSaveBtn.addEventListener("click", () => void explicitSave());
  window.addEventListener("keydown", (e) => {
    if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "s") return;
    e.preventDefault();
    if (!dom.notesSave.hidden) void explicitSave();
  });
  updateNotesSaveVisibility();
}
