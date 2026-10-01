import { dom } from "../lib/dom";
import { storeGet, storeSet } from "../lib/storage";

const MIN_PANEL_WIDTH = 180;
const KEY_STEP = 20;
const DEFAULT_WIDTH = { history: 240, questions: 340 };

type PanelSide = "history" | "questions";

const cssVar = (side: PanelSide) => `--${side}-w`;
const storageKey = (side: PanelSide) => `roofle-${side}-w`;

function restoreWidths(): void {
  for (const side of ["history", "questions"] as const) {
    const width = storeGet(storageKey(side));
    if (width) dom.workspace.style.setProperty(cssVar(side), `${width}px`);
  }
}

function clampWidth(width: number): number {
  const max = dom.workspace.getBoundingClientRect().width * 0.5;
  return Math.max(MIN_PANEL_WIDTH, Math.min(width, max));
}

function currentWidth(side: PanelSide): number {
  return parseFloat(dom.workspace.style.getPropertyValue(cssVar(side))) || 0;
}

function setupResizer(handle: HTMLElement): void {
  const side = handle.dataset.resizer as PanelSide;
  let dragging = false;

  handle.addEventListener("pointerdown", (e) => {
    dragging = true;
    handle.classList.add("dragging");
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    handle.setPointerCapture?.(e.pointerId);
  });

  window.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const rect = dom.workspace.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const width = side === "history" ? x : rect.width - x;
    dom.workspace.style.setProperty(cssVar(side), `${clampWidth(width)}px`);
  });

  const finish = () => {
    if (!dragging) return;
    dragging = false;
    handle.classList.remove("dragging");
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    const width = currentWidth(side);
    if (width) storeSet(storageKey(side), String(Math.round(width)));
  };
  window.addEventListener("pointerup", finish);
  window.addEventListener("pointercancel", finish);

  handle.addEventListener("keydown", (e) => {
    const grow = side === "history" ? KEY_STEP : -KEY_STEP;
    let delta: number;
    if (e.key === "ArrowRight") delta = grow;
    else if (e.key === "ArrowLeft") delta = -grow;
    else return;
    e.preventDefault();
    const width = clampWidth((currentWidth(side) || DEFAULT_WIDTH[side]) + delta);
    dom.workspace.style.setProperty(cssVar(side), `${width}px`);
    storeSet(storageKey(side), String(width));
  });
}

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
    if (side === "questions") dom.questionsRail.addEventListener("click", toggle);
  }
}

export function initLayout(): void {
  restoreWidths();
  initPanelToggles();
  document.querySelectorAll<HTMLElement>(".resizer").forEach(setupResizer);
  initFloatingControl();
}
