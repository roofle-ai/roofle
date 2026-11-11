import { invoke } from "@tauri-apps/api/core";

import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { revealTranscriptSource, SOURCE_EVENT } from "./notes-tabs";
import type { QuestionView } from "../types";

let questions: QuestionView[] = [];
/** Question whose source is spotlit in the transcription, marked in the list. */
let viewingSourceOf: number | null = null;
/** Ids already shown, so only genuinely new cards play the entrance animation. */
const seen = new Set<number>();
let followupsEnabled = true;

document.addEventListener(SOURCE_EVENT, (e) => {
  viewingSourceOf = (e as CustomEvent<number | null>).detail;
  dom.questionsList.querySelectorAll<HTMLElement>(".qcard").forEach((card) => {
    card.classList.toggle("is-viewing", card.dataset.qid === String(viewingSourceOf));
  });
});

/** Open questions first (newest first), then the ones the conversation already answered. Stale ones are dropped. */
function visibleQuestions(): QuestionView[] {
  const byNewest = (a: QuestionView, b: QuestionView) => b.id - a.id;
  const open = questions.filter((q) => q.status === "open").sort(byNewest);
  const answered = questions.filter((q) => q.status === "answered").sort(byNewest);
  return [...open, ...answered];
}

/** Reconcile the backend's full question list into the card, by id. */
export function ingestChallenges(incoming: QuestionView[]): void {
  if (incoming.length === 0) return;
  questions = incoming;
  renderQuestions();
}

function renderQuestions(): void {
  const scrollTop = dom.questionsCard.scrollTop;
  const shown = visibleQuestions();
  dom.questionsList.replaceChildren(...shown.map(renderQuestion));
  dom.questionsCard.scrollTop = scrollTop;
  shown.forEach((q) => seen.add(q.id));
  syncQuestionsCard();
}

/** One plain text box. Clicking the text jumps to where in the transcription it came from. */
function renderQuestion(q: QuestionView): HTMLElement {
  const card = create("article", `qcard ${q.status}`);
  card.dataset.qid = String(q.id);
  if (!seen.has(q.id)) card.classList.add("is-new");
  if (q.id === viewingSourceOf) card.classList.add("is-viewing");

  const { source } = q;
  if (source) {
    const text = create("button", "qtext", q.text);
    text.type = "button";
    text.title = "Show where this came up";
    text.addEventListener("click", () => revealTranscriptSource(source, q.id));
    card.append(text);
  } else {
    card.append(create("p", "qtext", q.text));
  }
  if (q.status === "answered" && q.reason) card.title = q.reason;
  if (q.status === "open") card.append(renderHide(q));
  return card;
}

/** Small "×": hides the question from the list (not a CTA). */
function renderHide(q: QuestionView): HTMLElement {
  const btn = create("button", "qfeedback-btn", "×");
  btn.type = "button";
  btn.title = "Hide this question";
  btn.setAttribute("aria-label", "Hide this question");
  btn.addEventListener("click", () => void sendDismissed(q));
  return btn;
}

async function sendDismissed(q: QuestionView): Promise<void> {
  if (session.viewedId === null) return;
  // The card moves via the backend's list update; on failure it stays as is.
  await invoke<void>("question_feedback", { sessionId: session.viewedId, questionId: q.id, kind: "dismissed" }).catch(() => {});
}

export function resetQuestions(): void {
  questions = [];
  seen.clear();
  dom.questionsList.replaceChildren();
  syncQuestionsCard();
}

/** Push the on/off choice to the backend; failures leave the UI state as is. */
export function syncFollowups(): Promise<void> {
  return invoke<void>("set_followups_enabled", { enabled: followupsEnabled }).catch(() => {});
}

export const isFollowupsEnabled = (): boolean => followupsEnabled;

export function setFollowupsEnabled(on: boolean): void {
  followupsEnabled = on;
  void syncFollowups();
}

/** Switchable only for the session being recorded, as before. */
export const canToggleFollowups = (): boolean =>
  session.state !== "idle" && session.activeId !== null && session.viewedId === session.activeId;

/** The card belongs to the notes view: hidden behind the analysis dashboard and when empty. */
export function syncQuestionsCard(): void {
  dom.questionsCard.hidden = !dom.analysisDashboard.hidden || dom.questionsList.childElementCount === 0;
}
