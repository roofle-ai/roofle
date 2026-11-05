import { dom } from "../lib/dom";
import { storeGet, storeSet } from "../lib/storage";

const NARROW = window.matchMedia("(max-width: 960px)");

/** Header buttons collapse / restore a side panel; the choice persists across launches. */
function initPanelToggles(): void {
  const label = { history: "conversations", questions: "questions" };
  for (const side of ["history", "questions"] as const) {
    const btn = document.getElementById(`toggle-${side}`);
    if (!btn) continue;
    const key = `roofle-${side}-collapsed`;
    const apply = (collapsed: boolean) => {
      dom.workspace.classList.toggle(`${side}-collapsed`, collapsed);
      btn.setAttribute("aria-pressed", String(!collapsed));
      btn.setAttribute(
        "aria-label",
        `${collapsed ? "Show" : "Collapse"} ${label[side]} panel`,
      );
    };
    const toggle = () => {
      const collapsed = !dom.workspace.classList.contains(`${side}-collapsed`);
      apply(collapsed);
      storeSet(key, collapsed ? "1" : "0");
    };
    apply(storeGet(key) === "1");
    btn.addEventListener("click", toggle);
    (side === "questions" ? dom.questionsRail : dom.historyRail).addEventListener("click", toggle);
  }
}

/**
 * When the window is squeezed, collapse both side panels so the transcript
 * stays visible (#10). Restores each panel's saved preference once the window
 * is wide again. Manual toggles still work while narrow until the next resize.
 */
function initNarrowCollapse(): void {
  const sync = () => {
    for (const side of ["history", "questions"] as const) {
      const btn = document.getElementById(`toggle-${side}`);
      if (!btn) continue;
      const preferCollapsed = storeGet(`roofle-${side}-collapsed`) === "1";
      const collapsed = NARROW.matches || preferCollapsed;
      dom.workspace.classList.toggle(`${side}-collapsed`, collapsed);
      btn.setAttribute("aria-pressed", String(!collapsed));
      btn.setAttribute(
        "aria-label",
        `${collapsed ? "Show" : "Collapse"} ${side === "history" ? "conversations" : "questions"} panel`,
      );
    }
  };
  NARROW.addEventListener("change", sync);
  sync();
}

export function initLayout(): void {
  initPanelToggles();
  initNarrowCollapse();
}
