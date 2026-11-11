import { dom } from "../lib/dom";
import { storeGet, storeSet } from "../lib/storage";

const KEY = "roofle-questions-collapsed";

/** Header click folds the card down to its title row; the choice persists across launches. */
export function initQuestionsCard(): void {
  const apply = (collapsed: boolean) => {
    dom.questionsCard.classList.toggle("is-collapsed", collapsed);
    dom.questionsCardToggle.setAttribute("aria-expanded", String(!collapsed));
  };
  apply(storeGet(KEY) === "1");
  dom.questionsCardToggle.addEventListener("click", () => {
    const collapsed = !dom.questionsCard.classList.contains("is-collapsed");
    apply(collapsed);
    storeSet(KEY, collapsed ? "1" : "0");
  });
}
