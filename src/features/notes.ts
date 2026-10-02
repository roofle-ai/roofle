import DOMPurify from "dompurify";
import { marked } from "marked";
import { session } from "../app/state";
import { dom } from "../lib/dom";
import { setStatus, showToast } from "../lib/status";
import { saveNotes } from "../transcripts";

const AUTOSAVE_DELAY_MS = 800;
let saveTimer: number | undefined;
/** True when the edited notes differ from what was last persisted. */
let dirty = false;
/** Notes as last loaded or saved — what "Discard changes" restores. */
let savedText = "";

/** True while the rendered markdown view is showing instead of the editor. */
let previewing = false;

export function scrollNotesToEnd(): void {
  const el = previewing ? dom.notesPreview : dom.notesInput;
  el.scrollTop = el.scrollHeight;
}

function renderPreview(): void {
  const md = dom.notesInput.value;
  const html = md.trim()
    ? DOMPurify.sanitize(marked.parse(md, { async: false, breaks: true }))
    : `<p class="notes-preview-empty">Nothing to preview yet.</p>`;
  dom.notesPreview.innerHTML = html;
  // Links in notes must not navigate the app webview.
  dom.notesPreview.querySelectorAll("a").forEach((a) => {
    a.target = "_blank";
    a.rel = "noopener noreferrer";
  });
}

/** Switch between the markdown editor and its rendered preview. */
export function setNotesPreview(on: boolean): void {
  previewing = on;
  if (on) renderPreview();
  dom.notesInput.hidden = on;
  if (on) dom.notesEditing.classList.remove("on");
  dom.notesPreview.hidden = !on;
}

/** Called after notes are loaded from a saved session: show them rendered. */
export function showLoadedNotes(): void {
  savedText = dom.notesInput.value;
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  dirty = false;
  updateNotesSaveVisibility();
  setNotesPreview(dom.notesInput.value.trim() !== "");
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

export function refreshNotesTitle(date?: Date): void {
  dom.chatTitle.textContent = notesTitle(date);
  // Continuing a saved session keeps its original start time.
  if (date || session.viewedId === null || !dom.notesMeta.dateTime) renderNotesMeta(date ?? new Date());
}

export function clearNotes(): void {
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  dirty = false;
  dom.notesInput.value = "";
  savedText = "";
  setNotesPreview(false);
  session.titleFromTranscript = undefined;
  updateNotesSaveVisibility();
  renderNotesMeta(new Date());
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
    const text = dom.notesInput.value;
    await saveNotes(session.viewedId, text);
    savedText = text;
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

/** Throw away unsaved edits and go back to the last saved notes. */
function discardChanges(): void {
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  dom.notesInput.value = savedText;
  dirty = false;
  setNotesPreview(savedText.trim() !== "");
  dom.notesInput.blur();
  updateNotesSaveVisibility();
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
  if (previewing) renderPreview();
  scrollNotesToEnd();
  // Programmatic writes don't fire `input`.
  scheduleNotesSave();
}

export function initNotes(): void {
  dom.notesInput.addEventListener("input", scheduleNotesSave);
  // Click the rendered notes to edit; leaving the editor (blur / Esc) renders them again.
  dom.notesPreview.addEventListener("click", (e) => {
    if (session.state !== "idle") return;
    if ((e.target as Element).closest("a")) return;
    // Let people select/copy rendered text without dropping into the editor.
    if (!window.getSelection()?.isCollapsed) return;
    setNotesPreview(false);
    dom.notesInput.focus();
    const end = dom.notesInput.value.length;
    dom.notesInput.setSelectionRange(end, end);
    scrollNotesToEnd();
  });
  dom.notesInput.addEventListener("focus", () => {
    dom.notesEditing.classList.add("on");
  });
  dom.notesInput.addEventListener("blur", () => {
    dom.notesEditing.classList.remove("on");
    if (session.state === "idle" && dom.notesInput.value.trim() !== "") setNotesPreview(true);
  });
  dom.notesInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") dom.notesInput.blur();
  });
  dom.notesDiscardBtn.addEventListener("click", discardChanges);
  dom.notesSaveBtn.addEventListener("click", () => void explicitSave());
  window.addEventListener("keydown", (e) => {
    if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "s") return;
    e.preventDefault();
    if (!dom.notesSave.hidden) void explicitSave();
  });
  updateNotesSaveVisibility();
  renderNotesMeta(new Date());
}
