import { session } from "../app/state";
import { dom } from "../lib/dom";

/** The welcome text belongs to a fresh, empty notepad: nothing open, nothing recording, nothing typed. */
export function syncWelcome(): void {
  const show = session.state === "idle" && session.viewedId === null && dom.notesInput.value === "";
  dom.notesWelcome.hidden = !show;
  dom.notesPane.classList.toggle("has-welcome", show);
}

export function initWelcome(): void {
  dom.notesInput.addEventListener("input", syncWelcome);
  syncWelcome();
}
