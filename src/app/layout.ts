import { dom } from "../lib/dom";
import { storeGet, storeSet } from "../lib/storage";

/** The transport pill starts centered over the notes and can be dragged anywhere in the panel. */
function initFloatingControl(): void {
  const { floatingControl, transcriptPanel: panel } = dom;

  const center = () => {
    const pr = panel.getBoundingClientRect();
    floatingControl.style.left = `${Math.max(0, (pr.width - floatingControl.offsetWidth) / 2)}px`;
    floatingControl.style.top = `${Math.max(0, (pr.height - floatingControl.offsetHeight) / 2)}px`;
  };
  center();
  window.addEventListener("resize", center);

  let drag: { dx: number; dy: number } | null = null;
  floatingControl.addEventListener("pointerdown", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    const r = floatingControl.getBoundingClientRect();
    drag = { dx: e.clientX - r.left, dy: e.clientY - r.top };
    floatingControl.classList.add("dragging");
    floatingControl.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  });
  window.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const pr = panel.getBoundingClientRect();
    const maxX = Math.max(0, pr.width - floatingControl.offsetWidth);
    const maxY = Math.max(0, pr.height - floatingControl.offsetHeight);
    const x = e.clientX - pr.left - drag.dx;
    const y = e.clientY - pr.top - drag.dy;
    floatingControl.style.left = `${Math.max(0, Math.min(x, maxX))}px`;
    floatingControl.style.top = `${Math.max(0, Math.min(y, maxY))}px`;
  });
  const release = () => {
    if (!drag) return;
    drag = null;
    floatingControl.classList.remove("dragging");
  };
  window.addEventListener("pointerup", release);
  window.addEventListener("pointercancel", release);
}

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
      window.dispatchEvent(new Event("resize")); // re-center the floating control
    };
    apply(storeGet(key) === "1");
    btn.addEventListener("click", toggle);
    (side === "questions" ? dom.questionsRail : dom.historyRail).addEventListener("click", toggle);
  }
}

export function initLayout(): void {
  initPanelToggles();
  initFloatingControl();
}
