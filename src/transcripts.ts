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

export interface TranscriptSegment {
  id: number;
  session_id: number;
  seq: number;
  start_secs: number;
  text: string;
  created_at: string;
}

export interface SessionDetail {
  session: Session;
  segments: TranscriptSegment[];
  transcript: string; // essay-paragraph rendering of the segments
}

/** Recorded sessions, newest first, each with a first-words preview. */
export function listSessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_sessions");
}

/** One session with its segments and rendered paragraph transcript. */
export function getSession(id: number): Promise<SessionDetail | null> {
  return invoke<SessionDetail | null>("get_session", { id });
}

/** Delete a session; its segments cascade. */
export function deleteSession(id: number): Promise<void> {
  return invoke<void>("delete_session", { id });
}
