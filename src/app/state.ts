export type SessionState = "idle" | "recording" | "paused";

/** Mutable UI state shared across features. */
export const session = {
  state: "idle" as SessionState,
  viewedId: null as number | null,
  elapsedSec: 0,
  /** First words of the live transcription, used as the panel title. */
  titleFromTranscript: undefined as string | undefined,
};
