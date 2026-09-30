import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { formatTime } from "../lib/format";
import { refreshIcons } from "../lib/icons";
import { scrollNotesToEnd } from "./notes";
import type { QuestionView } from "../types";

interface FollowUp extends QuestionView {
  timestamp: number;
  resolved: boolean;
}

const LISTENING_HINT =
  "Roofle is listening… follow-up questions will appear here as the conversation unfolds.";

let questions: FollowUp[] = [];
let analysisMode = false;

const stateLabel = (resolved: boolean) => (resolved ? "Resolved" : "Unanswered");

const resolveIcon = (resolved: boolean) =>
  `<i data-lucide="${resolved ? "circle-check" : "circle-dot"}" aria-hidden="true"></i>`;

const cardFor = (id: number) => document.querySelector<HTMLElement>(`[data-qid="${id}"]`);

/** Reconcile the backend's full question list into the panel, by id. */
export function ingestChallenges(incoming: QuestionView[], timestamp: number): void {
  if (incoming.length === 0) return;
  const ids = new Set(incoming.map((q) => q.id));

  for (const q of questions) {
    if (!ids.has(q.id)) cardFor(q.id)?.remove();
  }
  questions = questions.filter((q) => ids.has(q.id));

  for (const q of incoming) {
    const existing = questions.find((x) => x.id === q.id);
    if (existing) {
      updateQuestion(existing, q);
    } else {
      const fresh: FollowUp = { ...q, timestamp, resolved: q.status !== "open" };
      questions.push(fresh);
      renderQuestion(fresh);
    }
  }
  dom.questionsEmpty.hidden = questions.length !== 0;
  updateLiveBadge();
  if (analysisMode) renderSummary();
}

function updateQuestion(existing: FollowUp, q: QuestionView): void {
  existing.text = q.text;
  existing.status = q.status;
  existing.resolved = q.status !== "open";
  const card = cardFor(q.id);
  if (!card) return;
  card.classList.toggle("resolved", existing.resolved);
  const label = card.querySelector<HTMLElement>(".q-state");
  if (label) label.textContent = stateLabel(existing.resolved);
}

function renderQuestion(q: FollowUp): void {
  dom.questionsEmpty.hidden = true;
  const card = create("article", "qcard");
  card.dataset.qid = String(q.id);

  const top = create("div", "qcard-top");
  top.append(create("span", "chip Open", "Open"), create("span", "timestamp", formatTime(q.timestamp)));

  const label = create("span", "q-state", stateLabel(q.resolved));
  const metaLeft = create("div", "qmeta-left");
  metaLeft.append(label);

  const jumpBtn = create("button", "qaction jump");
  jumpBtn.type = "button";
  jumpBtn.setAttribute("aria-label", "Jump to context in transcript");
  jumpBtn.innerHTML = `<i data-lucide="corner-down-right" aria-hidden="true"></i>`;
  jumpBtn.addEventListener("click", () => {
    dom.notesInput.focus();
    scrollNotesToEnd();
  });

  const resolveBtn = create("button", "qaction resolve");
  resolveBtn.type = "button";
  const paintResolveBtn = () => {
    resolveBtn.setAttribute("aria-label", q.resolved ? "Mark unanswered" : "Mark resolved");
    resolveBtn.innerHTML = resolveIcon(q.resolved);
  };
  paintResolveBtn();
  resolveBtn.addEventListener("click", () => {
    q.resolved = !q.resolved;
    card.classList.toggle("resolved", q.resolved);
    label.textContent = stateLabel(q.resolved);
    paintResolveBtn();
    refreshIcons();
    if (analysisMode) renderSummary();
  });

  const actions = create("div", "qactions");
  actions.append(jumpBtn, resolveBtn);
  const meta = create("div", "qmeta");
  meta.append(metaLeft, actions);

  card.append(top, create("p", "qtext", q.text), meta);
  dom.questionsList.prepend(card);
  refreshIcons();
  updateLiveBadge();
}

export function resetQuestions(): void {
  questions = [];
  dom.questionsList.querySelectorAll(".qcard").forEach((el) => el.remove());
  dom.summaryCard.hidden = true;
  dom.questionsEmpty.hidden = false;
  dom.questionsEmptyHint.textContent = LISTENING_HINT;
  dom.qLive.className = "live-badge";
  dom.qLive.textContent = "";
}

export function setQuestionsHint(): void {
  dom.questionsEmptyHint.textContent =
    session.state === "paused"
      ? "Paused — follow-up questions will resume when you resume recording."
      : LISTENING_HINT;
}

function updateLiveBadge(): void {
  const count = questions.length;
  if (analysisMode) {
    const unresolved = questions.filter((q) => !q.resolved).length;
    dom.qLive.className = "live-badge show analysis";
    dom.qLive.textContent = count === 0 ? "" : unresolved === 0 ? "All resolved" : `${unresolved} open`;
  } else {
    dom.qLive.className = "live-badge show live";
    dom.qLive.textContent = count === 0 ? "" : `${count} live`;
  }
}

function renderSummary(): void {
  if (questions.length === 0) {
    dom.summaryCard.hidden = true;
    return;
  }
  dom.summaryCard.hidden = false;
  const count = (status: FollowUp["status"]) => questions.filter((q) => q.status === status).length;
  const open = count("open");
  const answered = count("answered");
  const stale = count("stale");

  dom.summaryStats.replaceChildren();
  const addStat = (n: number, label: string) => {
    const stat = create("div", "stat");
    stat.append(create("b", undefined, String(n)), create("span", undefined, label));
    dom.summaryStats.append(stat);
  };
  addStat(questions.length, "questions");
  if (open) addStat(open, "open");
  if (answered) addStat(answered, "answered");
  if (stale) addStat(stale, "stale");

  dom.summaryNote.textContent = open
    ? `${open} follow-up ${open === 1 ? "question remains" : "questions remain"} open for the team.`
    : answered > 0
      ? "All follow-up questions answered."
      : "All follow-up questions closed.";
  updateLiveBadge();
}

export function setAnalysisMode(on: boolean): void {
  analysisMode = on;
  if (on) {
    renderSummary();
  } else {
    dom.summaryCard.hidden = true;
    updateLiveBadge();
  }
}

/** The questions panel belongs to the notes view; hide it behind the analysis dashboard. */
export function syncQuestionsPanel(): void {
  dom.questionsPanel.hidden = !dom.analysisDashboard.hidden;
}
