import { session } from "../app/state";
import { dom } from "../lib/dom";
import { isNotesEmpty, onNotesChange } from "./notes-editor";

/** The welcome text belongs to a fresh, empty notepad: nothing open, nothing recording, nothing typed. */
export function syncWelcome(): void {
  const show = session.state === "idle" && session.viewedId === null && isNotesEmpty();
  dom.notesWelcome.hidden = !show;
  dom.notesPane.classList.toggle("has-welcome", show);
}

export function initWelcome(): void {
  onNotesChange(syncWelcome);
  syncWelcome();
}
