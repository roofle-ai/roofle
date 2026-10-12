import { invoke } from "@tauri-apps/api/core";

import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { formatTime } from "../lib/format";
import { revealTranscriptSource, SOURCE_EVENT } from "./notes-tabs";
import { refreshIcons } from "../lib/icons";
import type { QuestionSource, QuestionView } from "../types";

interface FollowUp extends QuestionView {
  resolved: boolean;
}

const LISTENING_HINT =
  "Roofle listens for gaps, assumptions and open points, then suggests questions worth asking.";

let questions: FollowUp[] = [];
let analysisMode = false;
/** Question whose source is spotlit in the transcription, marked in the list. */
let viewingSourceOf: number | null = null;

document.addEventListener(SOURCE_EVENT, (e) => {
  viewingSourceOf = (e as CustomEvent<number | null>).detail;
  dom.questionsList.querySelectorAll<HTMLElement>(".qcard").forEach((card) => {
    card.classList.toggle("is-viewing", card.dataset.qid === String(viewingSourceOf));
  });
});
/** Collapsed "earlier" groups the user has opened; survives re-renders. */
const expandedGroups = new Set<Status>(["answered"]);
/** Ids already shown, so only genuinely new cards play the entrance animation. */
const seen = new Set<number>();

type Status = FollowUp["status"];

/** Display order and copy for each group of questions. */
const GROUPS: { status: Status; title: string; answerLabel?: string }[] = [
  { status: "open", title: "Suggestions" },
  { status: "answered", title: "Answered in conversation", answerLabel: "From the conversation" },
  { status: "stale", title: "No longer relevant", answerLabel: "Why" },
];

const statusIcon = (status: Status) =>
  status === "answered" ? "circle-check" : status === "stale" ? "clock" : "circle-dot";

/** Reconcile the backend's full question list into the panel, by id. */
export function ingestChallenges(incoming: QuestionView[]): void {
  if (incoming.length === 0) return;
  questions = incoming.map((q) => ({ ...q, resolved: q.status !== "open" }));
  renderQuestions();
  updateLiveBadge();
}

/** Rebuild the grouped list. Lists are short, so a full render keeps it simple and consistent. */
function renderQuestions(): void {
  dom.questionsEmpty.hidden = questions.length !== 0;
  const scrollTop = dom.questionsList.scrollTop;
  dom.questionsList.replaceChildren();

  for (const group of GROUPS) {
    // Newest first within a group.
    const items = questions.filter((q) => q.status === group.status).sort((x, y) => y.id - x.id);
    if (items.length === 0) continue;

    // Open suggestions stay in view; earlier (answered / stale) ones are collapsed.
    const earlier = group.status !== "open";
    const section = create(earlier ? "details" : "section", `qgroup ${group.status}`);
    const head = create(earlier ? "summary" : "h3", "qgroup-title");
    head.append(create("span", undefined, group.title), create("span", "qgroup-count", String(items.length)));
    section.append(head);
    if (section instanceof HTMLDetailsElement) {
      section.open = expandedGroups.has(group.status);
      section.addEventListener("toggle", () =>
        section.open ? expandedGroups.add(group.status) : expandedGroups.delete(group.status),
      );
    }
    for (const q of items) section.append(renderQuestion(q, group.answerLabel));
    dom.questionsList.append(section);
  }
  dom.questionsList.scrollTop = scrollTop;
  questions.forEach((q) => seen.add(q.id));
  refreshIcons();
}

function renderQuestion(q: FollowUp, answerLabel = ""): HTMLElement {
  const card = create("article", `qcard ${q.status}`);
  card.dataset.qid = String(q.id);
  if (!seen.has(q.id)) card.classList.add("is-new");
  if (q.id === viewingSourceOf) card.classList.add("is-viewing");

  const head = create("div", "qcard-head");
  const icon = create("i");
  icon.dataset.lucide = statusIcon(q.status);
  icon.setAttribute("aria-hidden", "true");
  head.append(icon, create("p", "qtext", q.text));
  if (q.status === "open") head.append(renderHide(q));
  card.append(head);

  if (q.source) card.append(renderSource(q, q.source));

  if (q.reason) {
    const answer = create("div", "qanswer");
    answer.append(create("span", "qanswer-label", answerLabel), create("p", "qanswer-text", q.reason));
    if (q.answer_source) answer.append(renderSource(q, q.answer_source, "Answer source"));
    card.append(answer);
  }
  return card;
}

/** Small "×": hides the question from the list (not a CTA). */
function renderHide(q: FollowUp): HTMLElement {
  const btn = create("button", "qfeedback-btn", "×");
  btn.type = "button";
  btn.title = "Hide this question";
  btn.setAttribute("aria-label", "Hide this question");
  btn.addEventListener("click", () => void sendDismissed(q));
  return btn;
}

async function sendDismissed(q: FollowUp): Promise<void> {
  if (session.viewedId === null) return;
  // The card moves via the backend's list update; on failure it stays as is.
  await invoke<void>("question_feedback", { sessionId: session.viewedId, questionId: q.id, kind: "dismissed" }).catch(() => {});
}

/** Compact button that jumps to the transcript passage the question came from. */
function renderSource(q: FollowUp, src: QuestionSource, label = "Source"): HTMLElement {
  const btn = create("button", "qsource");
  btn.type = "button";
  const icon = create("i");
  icon.dataset.lucide = "audio-lines";
  icon.setAttribute("aria-hidden", "true");
  btn.append(icon, create("span", undefined, label), create("span", "qsource-time", formatTime(src.start_ms / 1000)));
  btn.title = `Show where in the transcription this ${label === "Source" ? "question came from" : "was answered"}`;
  btn.addEventListener("click", () => revealTranscriptSource(src, q.id));
  return btn;
}

export function resetQuestions(): void {
  questions = [];
  seen.clear();
  dom.questionsList.replaceChildren();
  dom.questionsEmpty.hidden = false;
  dom.questionsEmptyHint.textContent = LISTENING_HINT;
  dom.qLive.className = "live-badge";
  dom.qLive.textContent = "";
  dom.qRailBadge.hidden = true;
  dom.questionsRail.classList.remove("is-live");
}

let followupsEnabled = true;

/** Push the toggle's value to the backend; failures leave the UI state as is. */
export function syncFollowups(): Promise<void> {
  return invoke<void>("set_followups_enabled", { enabled: followupsEnabled }).catch(() => {});
}

export function initFollowupToggle(): void {
  dom.followupToggle.addEventListener("change", () => {
    followupsEnabled = dom.followupToggle.checked;
    void syncFollowups();
    setQuestionsHint();
  });
}

const viewingActiveSession = () =>
  session.state !== "idle" && session.activeId !== null && session.viewedId === session.activeId;

/** Enabled only while the session being recorded is the one on screen. */
export function updateFollowupToggle(): void {
  dom.followupToggle.disabled = !viewingActiveSession();
}

export function setQuestionsHint(): void {
    updateFollowupToggle();
  if (viewingActiveSession() && !followupsEnabled) {
    dom.questionsEmptyHint.textContent = "Suggestions are turned off for this session.";
    return;
  }
  dom.questionsEmptyHint.textContent =
    session.state === "paused"
      ? "Recording is paused. Suggestions resume when you do."
      : LISTENING_HINT;
}

function updateLiveBadge(): void {
  const count = questions.length;
  // Collapsed-rail badge: unresolved questions (all of them while live).
  const pending = analysisMode ? questions.filter((q) => !q.resolved).length : count;
  dom.qRailBadge.hidden = pending === 0;
  dom.qRailBadge.textContent = pending > 9 ? "9+" : String(pending);
  dom.questionsRail.classList.toggle("is-live", !analysisMode && count > 0);
  if (analysisMode) {
    const unresolved = questions.filter((q) => !q.resolved).length;
    dom.qLive.className = "live-badge show analysis";
    dom.qLive.textContent = count === 0 ? "" : unresolved === 0 ? "All answered" : `${unresolved} to ask`;
  } else {
    dom.qLive.className = "live-badge show live";
    dom.qLive.textContent = count === 0 ? "" : `${count} suggested`;
  }
}

export function setAnalysisMode(on: boolean): void {
  analysisMode = on;
  updateLiveBadge();
}

/** The questions panel belongs to the notes view; hide it behind the analysis dashboard. */
export function syncQuestionsPanel(): void {
  dom.questionsPanel.hidden = !dom.analysisDashboard.hidden;
}
