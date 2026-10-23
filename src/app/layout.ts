import { dom } from "../lib/dom";
import { storeGet, storeSet } from "../lib/storage";

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

export function initLayout(): void {
  initPanelToggles();
}
