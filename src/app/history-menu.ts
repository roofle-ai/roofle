import { dom } from "../lib/dom";

/** Clicks on these inside the menu act on it (search, delete) rather than leave it. */
const STAYS_OPEN = ".history-search, .history-delete, .topic-bucket-delete, .topic-bucket-head, .history-group-label";

function setOpen(open: boolean): void {
  dom.historyMenu.hidden = !open;
  dom.crumbBtn.setAttribute("aria-expanded", String(open));
  if (open) dom.historySearch.focus();
}

const isOpen = () => !dom.historyMenu.hidden;

/** The breadcrumb opens the conversations + settings dropdown; picking an entry, Esc or an outside click closes it. */
export function initHistoryMenu(): void {
  dom.crumbBtn.addEventListener("click", () => setOpen(!isOpen()));

  dom.historyMenu.addEventListener("click", (e) => {
    const t = e.target as HTMLElement;
    if (t.closest(STAYS_OPEN)) return;
    if (t.closest(".history-item, #new-session-btn, #settings-btn")) setOpen(false);
  });

  document.addEventListener("click", (e) => {
    const t = e.target as Node;
    if (isOpen() && !dom.historyMenu.contains(t) && !dom.crumbBtn.contains(t)) setOpen(false);
  });

  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !isOpen()) return;
    setOpen(false);
    dom.crumbBtn.focus();
  });
}
