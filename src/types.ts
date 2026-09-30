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
  seq: number;
  text: string;
}

export interface QuestionView {
  id: number;
  text: string;
  status: "open" | "answered" | "stale";
  /** Agent-written resolution: the answer when `answered`, the reason when `stale`. */
  reason: string | null;
}

export interface ChallengeUpdate {
  seq: number;
  questions: QuestionView[];
}

export interface TitleUpdate {
  session_id: number;
  title: string;
}
