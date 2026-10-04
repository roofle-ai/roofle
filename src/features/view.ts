import { clearNotes } from "./notes";
import { hideDashboard } from "./analysis";
import { hideRecap } from "./recap";
import { resetNotesTabs } from "./notes-tabs";
import { resetQuestions, setAnalysisMode } from "./questions";

/** Blank the notes, transcription, questions and analysis panes for a new or switched session. */
export function resetView(): void {
  clearNotes();
  resetNotesTabs();
  resetQuestions();
  setAnalysisMode(false);
  hideDashboard();
  hideRecap();
}
