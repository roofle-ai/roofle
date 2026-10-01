// Importing the shell first guarantees its markup exists before the queries below.
import "../ui";

function query<T extends Element>(selector: string): T {
  return document.querySelector<T>(selector)!;
}

export const dom = {
  recordBtn: query<HTMLButtonElement>("#record-btn"),
  recordIconPlay: query<SVGElement>("#record-icon-play"),
  recordIconPause: query<SVGElement>("#record-icon-pause"),
  stopBtn: query<HTMLButtonElement>("#stop-btn"),
  timer: query<HTMLSpanElement>("#timer"),
  sessionPill: query<HTMLSpanElement>("#session-pill"),
  statusDot: query<HTMLSpanElement>("#status-dot"),
  statusText: query<HTMLSpanElement>("#status-text"),
  notesInput: query<HTMLTextAreaElement>("#notes-input"),
  notesSave: query<HTMLDivElement>("#notes-save"),
  notesSaveBtn: query<HTMLButtonElement>("#notes-save-btn"),
  modelProgress: query<HTMLElement>("#model-progress"),
  modelProgressFill: query<HTMLDivElement>("#model-progress-fill"),
  modelProgressText: query<HTMLSpanElement>("#model-progress-text"),
  toast: query<HTMLDivElement>("#toast"),
  floatingControl: query<HTMLDivElement>("#floating-control"),
  transcriptPanel: query<HTMLElement>("#transcript-panel"),
  newSessionBtn: query<HTMLButtonElement>("#new-session-btn"),
  settingsBtn: query<HTMLButtonElement>("#settings-btn"),
  historyList: query<HTMLDivElement>(".history-list"),
  historyCount: query<HTMLSpanElement>("#history-count"),
  historyEmpty: query<HTMLDivElement>("#history-empty"),
  chatTitle: query<HTMLHeadingElement>("#chat-title"),
  themeToggle: query<HTMLButtonElement>("#theme-toggle"),
  workspace: query<HTMLDivElement>(".workspace"),
  questionsList: query<HTMLDivElement>("#questions-list"),
  questionsEmpty: query<HTMLDivElement>("#questions-empty"),
  questionsEmptyHint: query<HTMLParagraphElement>("#questions-empty-hint"),
  questionsPanel: query<HTMLElement>("#questions-panel"),
  questionsRail: query<HTMLButtonElement>("#questions-rail"),
  qRailBadge: query<HTMLSpanElement>("#q-rail-badge"),
  qLive: query<HTMLSpanElement>("#q-live"),
  summaryCard: query<HTMLDivElement>("#summary-card"),
  summaryStats: query<HTMLDivElement>("#summary-stats"),
  assistControl: query<HTMLDivElement>("#assist-control"),
  assistBtn: query<HTMLButtonElement>("#assist-btn"),
  assistMenu: query<HTMLDivElement>("#assist-menu"),
  assistReviewItem: query<HTMLButtonElement>('[data-assist="review"]'),
  assistReviewLabel: query<HTMLSpanElement>('[data-assist="review"] .assist-item-label'),
  analyseModal: query<HTMLDivElement>("#analyse-modal"),
  analyseClose: query<HTMLButtonElement>("#analyse-close"),
  analyseCancel: query<HTMLButtonElement>("#analyse-cancel"),
  analyseSubmit: query<HTMLButtonElement>("#analyse-submit"),
  perspectiveSelect: query<HTMLSelectElement>("#analyse-perspective"),
  contextSelect: query<HTMLSelectElement>("#analyse-context"),
  analysisDashboard: query<HTMLDivElement>("#analysis-dashboard"),
  analysisBody: query<HTMLDivElement>("#analysis-body"),
  analysisClose: query<HTMLButtonElement>("#analysis-close"),
};

export function create<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
