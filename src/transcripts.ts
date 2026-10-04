// Typed access to the Rust-side SQLite store.
//
// The DB lives in the backend (app data dir); the frontend only consumes it
// through these invoke wrappers. Models mirror roofle_core::storage.

import { invoke } from "@tauri-apps/api/core";

export interface Session {
  id: number;
  title: string;
  source: string;
  model_id: string;
  started_at: string; // RFC 3339 UTC
  ended_at: string | null;
  duration_secs: number;
  status: "recording" | "completed";
  updated_at: string; // last activity; advances when a session is continued
}

/** A session as listed in the history sidebar. */
export interface SessionSummary extends Session {
  preview: string; // first words of the transcript
}

export interface AudioSegment {
  id: number;
  session_id: number;
  source: "mic" | "system";
  text: string;
  start_ms: number;
  end_ms: number;
  confidence: number | null;
  created_at: string;
}

export interface SessionDetail {
  session: Session;
  segments: AudioSegment[];
  transcript: string; // essay-paragraph rendering of the segments
  notes: string; // the editable notes document (seed + user edits)
}

/** Recorded sessions, newest first, each with a first-words preview. */
export function listSessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_sessions");
}

/** One session with its source-tagged audio segments and rendered transcript. */
export function getSession(id: number): Promise<SessionDetail | null> {
  return invoke<SessionDetail | null>("get_session", { id });
}

/** Persist the editable notes document for a session. */
export function saveNotes(id: number, notes: string): Promise<void> {
  return invoke<void>("save_notes", { id, notes });
}

/** Delete a session; its segments cascade. */
export function deleteSession(id: number): Promise<void> {
  return invoke<void>("delete_session", { id });
}

/** A session's persisted follow-up question, as shown in the right panel. */
export interface SessionQuestion {
  id: number;
  text: string;
  status: "open" | "answered" | "stale";
  /** Agent-written resolution: the answer when `answered`, the reason when `stale`. */
  reason: string | null;
}

/** A session's persisted follow-up questions (for the right panel on open). */
export function listSessionQuestions(id: number): Promise<SessionQuestion[]> {
  return invoke<SessionQuestion[]>("list_session_questions", { id });
}

// --- Offline self-assessment (offline-agent) --------------------------------

/** One scored call dimension (0-100; `score` null when there was no evidence). */
export interface AnalysisDimension {
  score: number | null;
  reasoning: string;
  evidence: string[];
}

/** A full self-assessment returned by the offline-agent. */
export interface AnalysisResult {
  call_summary: string;
  caveats: string[];
  dimensions: Record<string, AnalysisDimension>;
  lowest_scoring_dimension: string | null;
  overall_score: number;
}

/** Evaluate a finished session from a perspective + context; returns the dashboard. */
export function analyseSession(
  id: number,
  perspective: string,
  context: string,
  apiKey: string,
): Promise<AnalysisResult> {
  return invoke<AnalysisResult>("analyse_session", { id, perspective, context, apiKey });
}

/** A persisted offline analysis for a session (context it was run with + result). */
export interface AnalysisView {
  context: string;
  result: AnalysisResult;
}

/** The session's persisted offline analysis, if one exists. */
export function getSessionAnalysis(id: number): Promise<AnalysisView | null> {
  return invoke<AnalysisView | null>("get_session_analysis", { id });
}

/** A short catch-up on the most recent part of a conversation. */
export interface Recap {
  summary: string;
  /** Seconds covered: the requested window, or less in a young conversation. */
  covered_secs: number;
}

/** Recap the last `windowSecs` of a session; null when nothing has been said yet. */
export function recapSession(id: number, windowSecs: number, apiKey: string): Promise<Recap | null> {
  return invoke<Recap | null>("recap_session", { id, windowSecs, apiKey });
}
