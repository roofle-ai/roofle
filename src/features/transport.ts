import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

import { loadStoredApiKey } from "../ai";
import { ensureLicensed, showLicenseModal } from "../license";
import { session, type SessionState } from "../app/state";
import { confirmDialog } from "../lib/confirm";
import { dom } from "../lib/dom";
import { describeError, setStatus } from "../lib/status";
import { loadLiveSessionSettings } from "../settings";
import { updateAnalyseVisibility } from "./analysis";
import { walkthroughDone } from "./walkthrough";
import { adoptActiveSession, loadHistory, syncTransportPlacement } from "./history";
import { refreshNotesTitle, saveNotesNow, setNotesPreview } from "./notes";
import { ensureModel } from "./model-setup";
import { markWrapUpPending, syncWrapUp } from "./wrap-up";
import { askCallSetup, type CallSetup } from "./call-setup";
import { resetQuestions, setAnalysisMode, initFollowupToggle, setQuestionsHint, syncFollowups } from "./questions";
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

const IDLE_NOTES_PLACEHOLDER = "Start recording or type your notes here…";

/** Empty notes during a session: say when they arrive (the user's interval) and where to look meanwhile. */
function liveNotesPlaceholder(): string {
  const mins = loadLiveSessionSettings().notesWindowMins;
  const unit = mins === 1 ? "minute" : "minutes";
  return `Notes appear after ${mins} ${unit} of conversation, then refresh every ${mins}. Want them sooner? Lower "Notes update interval" in Settings → Live Session. Meanwhile, follow along in the Transcription tab.`;
}

export function setSessionState(state: SessionState): void {
  session.state = state;
  if (state === "idle") {
    if (session.activeId !== null) markWrapUpPending(session.activeId);
    session.activeId = null;
  }
  else session.activeId ??= session.viewedId;

  dom.recordBtn.classList.toggle("recording", state === "recording");
  dom.recordBtn.classList.toggle("paused", state === "paused");
  dom.recordBtn.setAttribute("aria-label", RECORD_LABEL[state]);
  // The play glyph is only the resume affordance while paused; recording starts
  // from "New conversation" (or the tray / Cmd+R).
  dom.recordIconPlay.toggleAttribute("hidden", state !== "paused");
  dom.recordIconPause.toggleAttribute("hidden", state !== "recording");
  dom.stopBtn.hidden = state === "idle";

  // Live text is appended to the editor, so it must be showing while a session is active.
  if (state !== "idle") setNotesPreview(false);
  // Shown only while the editor is empty, so it disappears once notes arrive.
  dom.notesInput.placeholder =
    state === "idle" ? IDLE_NOTES_PLACEHOLDER : liveNotesPlaceholder();

  if (state === "recording") startTimer();
  else stopTimer();
  setStatus(state, STATUS_TEXT[state]);

  setQuestionsHint();
  updateAnalyseVisibility();
  syncWrapUp();
  syncTransportPlacement();
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

/** Send the call's type and goal to the pipeline and notes agent of the session just started. */
async function applyCallSetup({ kind, callType, goal }: CallSetup): Promise<void> {
  const sessionId = session.activeId ?? session.viewedId;
  if (sessionId === null) return;
  await invoke("set_meeting_type", { sessionId, kind, callType });
  if (goal) await invoke("set_goal", { sessionId, text: goal });
}

async function start(): Promise<void> {
  await walkthroughDone(); // first time only: show "How Roofle works" before the call setup
  if (!(await ensureLicensed())) return;
  if (!(await ensureModel())) return;
  const setup = await askCallSetup();
  if (setup === null) return;
  try {
    const continueId = session.viewedId;
    if (continueId === null) {
      refreshNotesTitle();
      resetView();
      resetTimer();
    }
    resetQuestions();
    setAnalysisMode(false);
    const settings = loadLiveSessionSettings();
    await invoke("set_char_limit", { limit: settings.charLimit });
    await invoke("set_max_open_questions", { limit: settings.maxOpenQuestions });
    await invoke("set_notes_window_mins", { mins: settings.notesWindowMins });
    await invoke("start_recording", {
      source: "both",
      sessionId: continueId,
      apiKey: loadStoredApiKey(),
    });
    if (continueId === null) await adoptActiveSession();
    else refreshNotesTitle();
    await syncFollowups();
    await applyCallSetup(setup);
    setSessionState("recording");
  } catch (err) {
    if (String(err) === "license_required") showLicenseModal(); // backend refused: license lapsed mid-session
    else setStatus("error", describeError(err));
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

/** A new conversation never starts over a live one: the user must agree to stop it first. */
async function newSession(): Promise<void> {
  if (session.state !== "idle") {
    const ok = await confirmDialog(
      "Stop the current conversation?",
      "A conversation is still in progress. Stop it to start a new one.",
      "Stop and start new",
    );
    if (!ok) return;
    await stop();
  }
  session.viewedId = null;
  updateAnalyseVisibility();
  refreshNotesTitle();
  resetView();
  syncWrapUp();
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
  initFollowupToggle();
  initShortcuts();
  // Tray "New conversation": same flow as the record button, goal prompt included.
  void listen("start-requested", () => {
    void (session.state === "idle" ? start() : newSession());
  });
}
