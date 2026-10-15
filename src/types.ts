/** Mirrors roofle_core::asr::Segment. */
export interface Segment {
  kind: "Partial" | "Final";
  source: "mic" | "system";
  text: string;
}

export interface ModelProgress {
  id: string;
  downloaded: number;
  total: number;
}

export interface NotesUpdate {
  session_id: number;
  seq: number;
  text: string;
}

/** The transcript entry a question was raised from. */
export interface QuestionSource {
  seq: number;
  /** `you` (microphone) or `them` (system audio). */
  speaker: "you" | "them";
  text: string;
  start_ms: number;
}

export interface QuestionView {
  id: number;
  text: string;
  status: "open" | "answered" | "stale";
  /** Agent-written resolution: the answer when `answered`, the reason when `stale`. */
  reason: string | null;
  /** Where in the transcript the question came from; null if unknown. */
  source: QuestionSource | null;
  /** Where in the transcript it was answered; null unless answered by a later line. */
  answer_source: QuestionSource | null;
  /** The user's feedback, if any. */
  feedback: "asked" | "dismissed" | null;
}

export interface ChallengeUpdate {
  session_id: number;
  seq: number;
  questions: QuestionView[];
}

export interface TitleUpdate {
  session_id: number;
  title: string;
}

/** Mirrors live_agent::KeyFigures. */
export interface Figure {
  label: string;
  value: string;
}

export interface KeyFigures {
  dates: Figure[];
  numbers: Figure[];
  names: Figure[];
}

/** Emitted when the end-of-call notes pass is over (mirrors state::WrapUp). */
export interface WrapUp {
  session_id: number;
  /** False when the notes could not be brought up to date. */
  ready: boolean;
  action_items: number;
}

/** Mirrors offline_agent::ActionItem; null when the notes did not state it. */
export interface ActionItem {
  owner: string | null;
  task: string;
  due: string | null;
}

/** Mirrors offline_agent::Recipe. */
export type RecipeKind = "recap-email" | "slack-post";

/** Mirrors offline_agent::Draft; `subject` is set only for email. */
export interface Draft {
  subject: string | null;
  body: string;
}
