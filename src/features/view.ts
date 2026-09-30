import { clearNotes } from "./notes";
import { hideDashboard } from "./analysis";
import { resetQuestions, setAnalysisMode } from "./questions";

/** Blank the notes, questions and analysis panes for a new or switched session. */
export function resetView(): void {
  clearNotes();
  resetQuestions();
  setAnalysisMode(false);
  hideDashboard();
}
