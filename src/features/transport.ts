import { invoke } from "@tauri-apps/api/core";

import { loadStoredApiKey } from "../ai";
import { session, type SessionState } from "../app/state";
import { dom } from "../lib/dom";
import { describeError, setStatus } from "../lib/status";
import { updateAnalyseVisibility } from "./analysis";
import { adoptActiveSession, loadHistory } from "./history";
import { refreshNotesTitle, saveNotesNow } from "./notes";
import { resetQuestions, setAnalysisMode, setQuestionsHint } from "./questions";
import { resetTimer, startTimer, stopTimer } from "./timer";
import { resetView } from "./view";

const RECORD_LABEL: Record<SessionState, string> = {
  idle: "Start recording",
  recording: "Pause recording",
  paused: "Resume recording",
};

const STATUS_TEXT: Record<SessionState, string> = {
  idle: "Idle",
  recording: "Recording",
  paused: "Paused",
};

export function setSessionState(state: SessionState): void {
  session.state = state;

  dom.recordBtn.classList.toggle("recording", state === "recording");
  dom.recordBtn.classList.toggle("paused", state === "paused");
  dom.recordBtn.setAttribute("aria-label", RECORD_LABEL[state]);
  // The play glyph is only the resume affordance while paused; recording starts
  // from "New conversation" (or the tray / Cmd+R).
  dom.recordIconPlay.toggleAttribute("hidden", state !== "paused");
  dom.recordIconPause.toggleAttribute("hidden", state !== "recording");
  dom.floatingControl.classList.toggle("is-idle", state === "idle");
  dom.stopBtn.hidden = state === "idle";

  if (state === "recording") startTimer();
  else stopTimer();
  setStatus(state, STATUS_TEXT[state]);

  setQuestionsHint();
  updateAnalyseVisibility();
}

async function command(name: "pause_recording" | "resume_recording", next: SessionState): Promise<void> {
  try {
    await invoke(name);
    setSessionState(next);
  } catch (err) {
    setStatus("error", describeError(err));
  }
}

const pause = () => command("pause_recording", "paused");
const resume = () => command("resume_recording", "recording");

async function start(): Promise<void> {
  try {
    const continueId = session.viewedId;
    if (continueId === null) {
      refreshNotesTitle();
      resetView();
      resetTimer();
    }
    resetQuestions();
    setAnalysisMode(false);
    await invoke("start_recording", {
      source: "both",
      sessionId: continueId,
      apiKey: loadStoredApiKey(),
    });
    if (continueId === null) await adoptActiveSession();
    else refreshNotesTitle();
    setSessionState("recording");
  } catch (err) {
    setStatus("error", describeError(err));
  }
}

export async function stop(): Promise<void> {
  try {
    await invoke("stop_recording");
  } finally {
    setSessionState("idle");
    await saveNotesNow();
    setAnalysisMode(true);
    await loadHistory();
  }
}

async function toggleRecording(): Promise<void> {
  if (session.state === "idle") await start();
  else if (session.state === "recording") await pause();
  else await resume();
}

async function newSession(): Promise<void> {
  if (session.state !== "idle") await stop();
  session.viewedId = null;
  updateAnalyseVisibility();
  refreshNotesTitle();
  resetView();
  await start();
}

function initShortcuts(): void {
  window.addEventListener("keydown", (e) => {
    if (!(e.metaKey || e.ctrlKey)) return;
    const key = e.key.toLowerCase();
    if (key === "r") {
      e.preventDefault();
      void toggleRecording();
    } else if (key === " ") {
      e.preventDefault();
      if (session.state === "recording") void pause();
      else if (session.state === "paused") void resume();
    } else if (key === ".") {
      e.preventDefault();
      if (session.state !== "idle") void stop();
    }
  });
}

export function initTransport(openSettings: () => void): void {
  dom.recordBtn.addEventListener("click", () => void toggleRecording());
  dom.stopBtn.addEventListener("click", () => void stop());
  dom.newSessionBtn?.addEventListener("click", () => void newSession());
  dom.settingsBtn?.addEventListener("click", openSettings);
  initShortcuts();
}
