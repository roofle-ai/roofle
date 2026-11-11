import { session } from "../app/state";
import { clearNotes } from "./notes";
import { hideDashboard } from "./analysis";
import { hideAssistCard } from "./assist-prompts";
import { hideRecap } from "./recap";
import { resetNotesTabs } from "./notes-tabs";
import { resetQuestions } from "./questions";

/** Blank the notes, transcription, questions and analysis panes for a new or switched session. */
export function resetView(): void {
  session.hasTranscript = false;
  clearNotes();
  resetNotesTabs();
  resetQuestions();
  hideDashboard();
  hideRecap();
  hideAssistCard();
}
