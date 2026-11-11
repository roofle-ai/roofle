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

const byNewest = (a: QuestionView, b: QuestionView) => b.id - a.id;
const withStatus = (status: QuestionView["status"]) =>
  questions.filter((q) => q.status === status).sort(byNewest);

/** Reconcile the backend's full question list into the card, by id. */
export function ingestChallenges(incoming: QuestionView[]): void {
  if (incoming.length === 0) return;
  questions = incoming;
  renderQuestions();
}

/** Open questions, then answered ones (with their answer), then the stale ones as small chips. */
function renderQuestions(): void {
  const scrollTop = dom.questionsCard.scrollTop;
  const cards = [...withStatus("open"), ...withStatus("answered")];
  const stale = withStatus("stale");
  dom.questionsList.replaceChildren(...cards.map(renderQuestion), ...(stale.length ? [renderStale(stale)] : []));
  dom.questionsCard.scrollTop = scrollTop;
  [...cards, ...stale].forEach((q) => seen.add(q.id));
  syncQuestionsCard();
}

/** A question's text; clicking it jumps to where in the transcription it came up. */
function renderText(q: QuestionView, className: string): HTMLElement {
  const { source } = q;
  if (!source) return create("p", className, q.text);
  const text = create("button", className, q.text);
  text.type = "button";
  text.title = "Show where this came up";
  text.addEventListener("click", () => revealTranscriptSource(source, q.id));
  return text;
}

function renderQuestion(q: QuestionView): HTMLElement {
  const card = create("article", `qcard ${q.status}`);
  card.dataset.qid = String(q.id);
  if (!seen.has(q.id)) card.classList.add("is-new");
  if (q.id === viewingSourceOf) card.classList.add("is-viewing");
  if (q.status === "answered") card.append(create("span", "qtag", "Answered later in the conversation"));
  card.append(renderText(q, "qtext"));
  if (q.status === "open") card.append(renderHide(q));
  if (q.status === "answered" && q.reason) card.append(renderAnswer(q, q.reason));
  return card;
}

/** What the conversation said; clicking it jumps to where it was said. */
function renderAnswer(q: QuestionView, reason: string): HTMLElement {
  const where = q.answer_source;
  if (!where) return create("p", "qanswer", reason);
  const text = create("button", "qanswer", reason);
  text.type = "button";
  text.title = "Show where this was answered";
  text.addEventListener("click", () => revealTranscriptSource(where, q.id));
  return text;
}

/** Questions that stopped mattering: a small label and one muted chip each (full text on hover). */
function renderStale(items: QuestionView[]): HTMLElement {
  const box = create("section", "qstale");
  box.append(create("p", "qstale-label", "No longer relevant"));
  const chips = create("div", "qstale-chips");
  for (const q of items) {
    const chip = renderText(q, "qstale-chip");
    chip.title = q.reason ? `${q.text}\n${q.reason}` : q.text;
    chips.append(chip);
  }
  box.append(chips);
  return box;
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
