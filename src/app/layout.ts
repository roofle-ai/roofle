import { dom } from "../lib/dom";
import { storeGet, storeSet } from "../lib/storage";

const NARROW = window.matchMedia("(max-width: 960px)");
const KEY = "roofle-history-collapsed";
const btn = () => document.getElementById("toggle-history");

function apply(collapsed: boolean): void {
  dom.workspace.classList.toggle("history-collapsed", collapsed);
  btn()?.setAttribute("aria-pressed", String(!collapsed));
  btn()?.setAttribute("aria-label", `${collapsed ? "Show" : "Collapse"} conversations panel`);
}

/** Header button and rail collapse / restore the history panel; the choice persists across launches. */
function toggle(): void {
  const collapsed = !dom.workspace.classList.contains("history-collapsed");
  apply(collapsed);
  storeSet(KEY, collapsed ? "1" : "0");
}

/** A squeezed window collapses the panel so the notes stay visible; the saved choice returns once wide again. */
function syncNarrow(): void {
  apply(NARROW.matches || storeGet(KEY) === "1");
}

export function initLayout(): void {
  btn()?.addEventListener("click", toggle);
  dom.historyRail.addEventListener("click", toggle);
  NARROW.addEventListener("change", syncNarrow);
  syncNarrow();
}
