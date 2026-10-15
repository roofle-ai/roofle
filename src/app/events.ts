import { listen } from "@tauri-apps/api/event";

import { adoptActiveSession, loadHistory } from "../features/history";
import { applyGeneratedNotes } from "../features/notes";
import { refreshTranscription } from "../features/notes-tabs";
import { ingestChallenges, setAnalysisMode } from "../features/questions";
import { setSessionState } from "../features/transport";
import { onWrapUp } from "../features/wrap-up";
import { dom } from "../lib/dom";
import { previewWords } from "../lib/format";
import { setStatus } from "../lib/status";
import type { ChallengeUpdate, ModelProgress, NotesUpdate, Segment, TitleUpdate, WrapUp } from "../types";
import { session } from "./state";

const MODEL_PROGRESS_HIDE_MS = 900;

function onTranscript({ kind, text }: Segment): void {
  if (kind === "Final") refreshTranscription();
  // Notes come from the agent, not raw transcription; only the title uses it.
  if (kind !== "Final" || session.titleFromTranscript || !text.trim()) return;
  session.titleFromTranscript = previewWords(text);
  dom.chatTitle.textContent = session.titleFromTranscript;
}

/** The agent renamed a finished session; reflect it in the panel and list. */
function onTitleUpdate({ session_id, title }: TitleUpdate): void {
  if (session.viewedId === session_id) {
    session.titleFromTranscript = title;
    dom.chatTitle.textContent = title;
  }
  void loadHistory();
}

function onModelProgress({ downloaded, total }: ModelProgress): void {
  if (total <= 0) return;
  const pct = Math.round((downloaded / total) * 100);
  setStatus("busy", "Preparing model…");
  dom.modelProgress.hidden = false;
  dom.modelProgressFill.style.width = `${pct}%`;
  dom.modelProgressText.textContent = `Downloading… ${pct}%`;
  if (pct < 100) return;
  window.setTimeout(() => {
    dom.modelProgress.hidden = true;
    dom.modelProgressFill.style.width = "0%";
    if (session.state === "idle") setStatus("idle", "Idle");
  }, MODEL_PROGRESS_HIDE_MS);
}

/** Stay in sync with recordings started or stopped from the tray menu. */
async function onRecordingState(recording: boolean): Promise<void> {
  if (recording) {
    await adoptActiveSession();
    setSessionState("recording");
  } else {
    setSessionState("idle");
    setAnalysisMode(true);
  }
  await loadHistory();
}

export async function initEvents(): Promise<void> {
  await listen<Segment>("transcript-update", (e) => onTranscript(e.payload));
  await listen<ChallengeUpdate>("challenge-update", (e) => {
    if (e.payload.session_id === session.viewedId) ingestChallenges(e.payload.questions);
  });
  await listen<NotesUpdate>("notes-update", (e) => {
    if (e.payload.session_id === session.viewedId) applyGeneratedNotes(e.payload.text);
  });
  await listen<WrapUp>("call-wrapped-up", (e) => onWrapUp(e.payload));
  await listen<ModelProgress>("model-progress", (e) => onModelProgress(e.payload));
  await listen<boolean>("recording-state", (e) => onRecordingState(e.payload));
  await listen<string>("app-status", (e) => setStatus("error", e.payload));
  await listen<TitleUpdate>("title-update", (e) => onTitleUpdate(e.payload));
}
