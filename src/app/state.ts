export type NotesTab = "notes" | "transcription";
export type SessionState = "idle" | "recording" | "paused";

/** Mutable UI state shared across features. */
export const session = {
  state: "idle" as SessionState,
  viewedId: null as number | null,
  /** The session being recorded (null while idle); may differ from `viewedId`. */
  activeId: null as number | null,
  /** Which view of the meeting the centre panel shows. */
  notesTab: "notes" as NotesTab,
  elapsedSec: 0,
  /** The viewed session has transcribed speech (so notes can be (re)generated). */
  hasTranscript: false,
  /** First words of the live transcription, used as the panel title. */
  titleFromTranscript: undefined as string | undefined,
};
