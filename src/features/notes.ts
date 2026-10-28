import { invoke } from "@tauri-apps/api/core";
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
/** Notes as last loaded or saved — what discarding edits restores. */
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
  dirty = false;
  dom.notesInput.value = "";
  savedText = "";
  setNotesPreview(false);
  session.titleFromTranscript = undefined;
  renderNotesMeta(new Date());
}

export function scheduleNotesSave(): void {
  window.clearTimeout(saveTimer);
  // A finished session saves explicitly (save prompt / ⌘S) once the user edits.
  if (session.state === "idle") {
    dirty = true;
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
    return true;
  } catch (err) {
    setStatus("error", String(err));
    return false;
  }
}

/** Whether a finished session's notes are open in the editor (as opposed to the rendered view). */
export function isEditingNotes(): boolean {
  return session.state === "idle" && session.viewedId !== null && !previewing;
}

/** Open a finished session's notes in the editor, caret at the end. */
export function startEditingNotes(): void {
  setNotesPreview(false);
  dom.notesEditing.classList.add("on");
  dom.notesInput.focus();
  const end = dom.notesInput.value.length;
  dom.notesInput.setSelectionRange(end, end);
  scrollNotesToEnd();
}

/** Back to the rendered notes. */
function stopEditingNotes(): void {
  dom.notesEditing.classList.remove("on");
  setNotesPreview(dom.notesInput.value.trim() !== "");
}

/** Leave the editor: straight out when nothing changed, otherwise ask what to do with the edits. */
function finishEditingNotes(): void {
  if (dirty) dom.saveNotesModal.hidden = false;
  else stopEditingNotes();
}

function closeSavePrompt(): void {
  dom.saveNotesModal.hidden = true;
}

/** Persist the edits and leave the editor. */
async function saveAndFinish(): Promise<void> {
  closeSavePrompt();
  if (!(await saveNotesNow())) return;
  showToast("Notes saved");
  stopEditingNotes();
}

/** Throw away unsaved edits and leave the editor. */
function discardAndFinish(): void {
  closeSavePrompt();
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  dom.notesInput.value = savedText;
  dirty = false;
  stopEditingNotes();
}

/** Replace the editor with the agent's full notes document and autosave it. */
export function applyGeneratedNotes(text: string): void {
  const notes = text.trim();
  if (!notes) return;
  dom.notesInput.value = notes;
  if (previewing) renderPreview();
  scrollNotesToEnd();
  // Programmatic writes don't fire `input`.
  if (session.state === "idle") showLoadedNotes(); // already saved by the backend
  else scheduleNotesSave();
}

export function initNotes(): void {
  dom.notesInput.addEventListener("input", scheduleNotesSave);
  dom.notesInput.addEventListener("focus", () => dom.notesEditing.classList.add("on"));
  dom.notesInput.addEventListener("blur", () => {
    // A finished session stays in edit mode until the user finishes; a live one only while focused.
    if (!isEditingNotes()) dom.notesEditing.classList.remove("on");
  });
  dom.notesInput.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (isEditingNotes()) finishEditingNotes();
    else dom.notesInput.blur();
  });

  // Pressing anywhere outside the notes panel finishes the edit. `pointerdown` (not `click`)
  // so the save prompt's own buttons, which hide it on click, can't re-trigger this.
  document.addEventListener("pointerdown", (e) => {
    if (!isEditingNotes() || !dom.saveNotesModal.hidden) return;
    if (!e.composedPath().includes(dom.transcriptPanel)) finishEditingNotes();
  });

  dom.saveNotesDiscard.addEventListener("click", discardAndFinish);
  dom.saveNotesSave.addEventListener("click", () => void saveAndFinish());
  dom.saveNotesModal.addEventListener("click", (e) => e.target === dom.saveNotesModal && closeSavePrompt());

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !dom.saveNotesModal.hidden) return closeSavePrompt();
    if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "s") return;
    e.preventDefault();
    if (isEditingNotes() && dirty) void saveAndFinish();
  });
  renderNotesMeta(new Date());
}
