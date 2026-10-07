import { session, type NotesTab } from "../app/state";
import { create, dom } from "../lib/dom";
import { setStatus } from "../lib/status";
import { getSession, type AudioSegment } from "../transcripts";
import type { QuestionSource } from "../types";
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

/** The passage a question was suggested from, spotlit in the transcription. */
let highlight: { source: QuestionSource; questionId: number } | null = null;
/** Scroll to the highlight on the next render (set when a source is opened). */
let revealPending = false;

/** Fired when the spotlight changes; `detail` is the active question id or null. */
export const SOURCE_EVENT = "roofle:source-highlight";

const announce = () =>
  document.dispatchEvent(
    new CustomEvent<number | null>(SOURCE_EVENT, { detail: highlight?.questionId ?? null }),
  );

/** A segment is the source when it was folded into the cited transcript-log entry. */
const isHighlighted = (s: AudioSegment): boolean =>
  highlight !== null && s.log_seq === highlight.source.seq;

/** Drop the spotlight (pill button, Esc, back to notes, session change). */
export function clearTranscriptSource(): void {
  if (!highlight) return;
  highlight = null;
  revealPending = false;
  dom.transcriptionList.classList.remove("has-source");
  dom.transcriptionList.querySelectorAll(".is-source").forEach((r) => r.classList.remove("is-source"));
  dom.transcriptionList.querySelectorAll(".source-pill").forEach((p) => p.remove());
  announce();
}

function sourcePill(): HTMLElement {
  const pill = create("span", "source-pill");
  const clear = create("button", "source-pill-clear");
  clear.type = "button";
  clear.setAttribute("aria-label", "Clear highlight");
  clear.textContent = "×";
  clear.addEventListener("click", clearTranscriptSource);
  pill.append("Source for question", clear);
  return pill;
}

function renderSegment(segment: AudioSegment): HTMLLIElement {
  const row = create("li", "transcription-row");
  if (isHighlighted(segment)) row.classList.add("is-source");
  row.append(
    create("span", `source-label source-${segment.source}`, SOURCE_LABELS[segment.source]),
    create("time", "transcription-time", formatOffset(segment.start_ms)),
    create("p", "transcription-text", segment.text),
  );
  return row;
}

/** Within this many px of the bottom counts as "following" the live transcript. */
const FOLLOW_THRESHOLD_PX = 48;

/** Follow new lines until the user scrolls up; scrolling back down resumes. */
let following = true;

function scrollToLatest(): void {
  const pane = dom.transcriptionPane;
  pane.scrollTop = pane.scrollHeight;
}

/** Replace the read-only list with the given segments. */
function renderTranscription(segments: AudioSegment[]): void {
  dom.transcriptionList.replaceChildren(...segments.map(renderSegment));
  dom.transcriptionEmpty.hidden = segments.length > 0;

  const first = dom.transcriptionList.querySelector<HTMLElement>(".is-source");
  dom.transcriptionList.classList.toggle("has-source", first !== null);
  first?.append(sourcePill());
  if (first && revealPending) {
    revealPending = false;
    // Wait a frame so the pane is laid out (it was just un-hidden), then centre the passage.
    requestAnimationFrame(() => {
      const pane = dom.transcriptionPane;
      const offset = first.getBoundingClientRect().top - pane.getBoundingClientRect().top;
      const top = pane.scrollTop + offset - pane.clientHeight / 4;
      pane.scrollTo({ top: Math.max(0, top), behavior: "smooth" });
    });
  } else if (following && !highlight) {
    scrollToLatest();
  }
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

/** Open the transcription tab with the passage a question came from spotlit. */
export function revealTranscriptSource(source: QuestionSource, questionId: number): void {
  highlight = { source, questionId };
  revealPending = true;
  following = false;
  announce();
  setNotesTab("transcription");
}

export function setNotesTab(tab: NotesTab): void {
  if (tab === "notes") clearTranscriptSource();
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
  clearTranscriptSource();
  setNotesTab("notes");
  renderTranscription([]);
}

export function initNotesTabs(): void {
  dom.transcriptionPane.addEventListener("scroll", () => {
    const pane = dom.transcriptionPane;
    if (pane.hidden) return;
    following = pane.scrollHeight - pane.scrollTop - pane.clientHeight <= FOLLOW_THRESHOLD_PX;
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && highlight && !dom.transcriptionPane.hidden) clearTranscriptSource();
  });
  dom.notesTabs.forEach((btn) =>
    btn.addEventListener("click", () => setNotesTab(btn.dataset.tab as NotesTab)),
  );
}
