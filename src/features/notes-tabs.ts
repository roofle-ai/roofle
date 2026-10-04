import { session, type NotesTab } from "../app/state";
import { create, dom } from "../lib/dom";
import { setStatus } from "../lib/status";
import { getSession, type AudioSegment } from "../transcripts";
import { updateNotesSaveVisibility } from "./notes";

const SOURCE_LABELS: Record<AudioSegment["source"], string> = {
  mic: "Mic",
  system: "System",
};

function formatOffset(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function renderSegment(segment: AudioSegment): HTMLLIElement {
  const row = create("li", "transcription-row");
  row.append(
    create("span", `source-label source-${segment.source}`, SOURCE_LABELS[segment.source]),
    create("time", "transcription-time", formatOffset(segment.start_ms)),
    create("p", "transcription-text", segment.text),
  );
  return row;
}

/** Replace the read-only list with the given segments. */
function renderTranscription(segments: AudioSegment[]): void {
  dom.transcriptionList.replaceChildren(...segments.map(renderSegment));
  dom.transcriptionEmpty.hidden = segments.length > 0;
}

/** Reload the transcription of the viewed session (no-op unless it's showing). */
export function refreshTranscription(): void {
  if (session.notesTab !== "transcription") return;
  const id = session.viewedId;
  if (id === null) return renderTranscription([]);
  getSession(id)
    .then((detail) => {
      if (session.viewedId === id) renderTranscription(detail?.segments ?? []);
    })
    .catch((err) => setStatus("error", String(err)));
}

export function setNotesTab(tab: NotesTab): void {
  session.notesTab = tab;
  const showingNotes = tab === "notes";
  dom.notesTabs.forEach((btn) => {
    const active = btn.dataset.tab === tab;
    btn.classList.toggle("active", active);
    btn.setAttribute("aria-selected", String(active));
  });
  dom.notesPane.hidden = !showingNotes;
  dom.transcriptionPane.hidden = showingNotes;
  updateNotesSaveVisibility();
  refreshTranscription();
}

/** A new or switched session always starts on its notes. */
export function resetNotesTabs(): void {
  setNotesTab("notes");
  renderTranscription([]);
}

export function initNotesTabs(): void {
  dom.notesTabs.forEach((btn) =>
    btn.addEventListener("click", () => setNotesTab(btn.dataset.tab as NotesTab)),
  );
}
