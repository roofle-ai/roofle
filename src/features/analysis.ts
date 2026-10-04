import { loadStoredApiKey } from "../ai";
import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { formatScore, humanizeKey } from "../lib/format";
import { analyseSession, getSessionAnalysis, type AnalysisResult } from "../transcripts";
import { syncQuestionsPanel } from "./questions";

const PERSPECTIVES = ["Sales rep", "Interviewer", "Presenter", "Facilitator"];
const CONTEXTS = ["Discovery", "Demo", "Interview", "Team meeting", "1:1"];

/** Assist is offered whenever a session is in view; the review needs it to be finished. */
export function updateAnalyseVisibility(): void {
  dom.assistControl.hidden = session.viewedId === null;
  dom.assistReviewItem.disabled = session.state !== "idle";
  void refreshReviewLabel();
}

/** The review action flips to "View conversation review" once the viewed session has a stored review. */
async function refreshReviewLabel(): Promise<void> {
  const id = session.viewedId;
  if (id === null || dom.assistControl.hidden) return;
  let stored = false;
  try {
    stored = (await getSessionAnalysis(id)) !== null;
  } catch {
    // Lookup failure keeps the default "Conversation review" label.
  }
  // Ignore if the user switched sessions while the lookup was in flight.
  if (session.viewedId !== id) return;
  dom.assistReviewLabel.textContent = stored ? "View conversation review" : "Conversation review";
}

export function hideDashboard(): void {
  dom.analysisDashboard.hidden = true;
  syncQuestionsPanel();
}

function showDashboard(): void {
  dom.analysisDashboard.hidden = false;
  syncQuestionsPanel();
}

/** Show a stored analysis if there is one; otherwise ask for perspective/context. */
async function onAnalyseClick(): Promise<void> {
  if (session.viewedId === null) return;
  try {
    const stored = await getSessionAnalysis(session.viewedId);
    if (stored) {
      showDashboard();
      renderAnalysis(stored.result);
      return;
    }
  } catch {
    // A failed lookup falls through to the modal.
  }
  fillSelect(dom.perspectiveSelect, PERSPECTIVES);
  fillSelect(dom.contextSelect, CONTEXTS);
  dom.analyseModal.hidden = false;
}

function closeModal(): void {
  dom.analyseModal.hidden = true;
}

function fillSelect(select: HTMLSelectElement, options: string[]): void {
  select.replaceChildren(
    ...options.map((o) => {
      const opt = create("option", undefined, o);
      opt.value = o;
      return opt;
    }),
  );
}

async function runAnalysis(): Promise<void> {
  if (session.viewedId === null) return;
  const perspective = dom.perspectiveSelect.value;
  const context = dom.contextSelect.value;
  closeModal();

  dom.analysisBody.replaceChildren(create("p", "analysis-loading", "Reviewing…"));
  showDashboard();

  try {
    const result = await analyseSession(session.viewedId, perspective, context, loadStoredApiKey());
    renderAnalysis(result);
    dom.assistReviewLabel.textContent = "View conversation review";
  } catch (err) {
    dom.analysisBody.replaceChildren(create("p", "analysis-error", String(err)));
  }
}

function renderAnalysis(r: AnalysisResult): void {
  const body = dom.analysisBody;
  body.replaceChildren();

  const dims = Object.entries(r.dimensions);
  if (!r.call_summary && dims.length === 0 && r.caveats.length === 0) {
    body.append(create("p", "analysis-error", "Not enough dialogue to review"));
    return;
  }

  const overall = create("div", "analysis-overall");
  overall.append(create("b", undefined, formatScore(r.overall_score)), create("span", undefined, "Overall score"));
  if (r.lowest_scoring_dimension) {
    overall.append(create("small", undefined, `Work on: ${humanizeKey(r.lowest_scoring_dimension)}`));
  }
  body.append(overall);

  if (r.call_summary) body.append(create("p", "analysis-summary", r.call_summary));

  if (dims.length > 0) {
    const grid = create("div", "analysis-scores");
    for (const [key, dim] of dims) {
      grid.append(renderDimension(key, dim, r.lowest_scoring_dimension === key));
    }
    body.append(grid);
  }

  appendList(body, "Caveats", r.caveats, "analysis-warn");
}

function renderDimension(key: string, dim: AnalysisResult["dimensions"][string], lowest: boolean): HTMLElement {
  const card = create("div", "analysis-score");
  card.classList.toggle("lowest", lowest);

  const head = create("div", "analysis-score-head");
  head.append(
    create("span", "analysis-score-name", humanizeKey(key)),
    create("span", "analysis-score-val", dim.score === null ? "—" : formatScore(dim.score)),
  );
  card.append(head);

  if (dim.score !== null) {
    const track = create("div", "analysis-score-track");
    const fill = create("div", "analysis-score-fill");
    fill.style.width = `${Math.max(0, Math.min(100, dim.score))}%`;
    track.append(fill);
    card.append(track);
  }
  if (dim.reasoning) card.append(create("p", "analysis-score-reason", dim.reasoning));
  if (dim.evidence.length > 0) {
    const list = create("ul", "analysis-evidence");
    list.append(...dim.evidence.map((e) => create("li", undefined, e)));
    card.append(list);
  }
  return card;
}

function appendList(parent: HTMLElement, title: string, items: string[], itemClass: string): void {
  if (items.length === 0) return;
  const list = create("ul", "analysis-list");
  list.append(...items.map((item) => create("li", itemClass, item)));
  parent.append(create("h4", "analysis-block-title", title), list);
}

export function closeAssistMenu(): void {
  dom.assistMenu.hidden = true;
  dom.assistBtn.setAttribute("aria-expanded", "false");
}

function toggleAssistMenu(): void {
  const open = dom.assistMenu.hidden;
  dom.assistMenu.hidden = !open;
  dom.assistBtn.setAttribute("aria-expanded", String(open));
}

export function initAnalysis(): void {
  dom.assistBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    toggleAssistMenu();
  });
  document.addEventListener("click", (e) => {
    if (!dom.assistControl.contains(e.target as Node)) closeAssistMenu();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeAssistMenu();
  });
  dom.assistReviewItem.addEventListener("click", () => {
    closeAssistMenu();
    void onAnalyseClick();
  });
  dom.analyseClose.addEventListener("click", closeModal);
  dom.analyseCancel.addEventListener("click", closeModal);
  dom.analyseSubmit.addEventListener("click", () => void runAnalysis());
  dom.analysisClose.addEventListener("click", hideDashboard);
}
