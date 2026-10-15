import { loadStoredApiKey } from "../ai";
import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { showToast } from "../lib/status";
import { keyFigures } from "../transcripts";
import type { Figure, KeyFigures } from "../types";
import { closeAssistMenu } from "./analysis";
import { hideRecap } from "./recap";

const NOTHING_SAID = "Nothing has been said yet. Try again once the conversation gets going.";
const NONE_FOUND = "No dates, numbers or names stood out so far.";

type FiguresState = "loading" | "ready" | "empty" | "error";

const GROUPS: ReadonlyArray<{ key: keyof KeyFigures; title: string }> = [
  { key: "dates", title: "Dates" },
  { key: "numbers", title: "Numbers" },
  { key: "names", title: "People and companies" },
];

/** What Copy puts on the clipboard. */
let copyText = "";

function render(state: FiguresState, meta = "", message = ""): void {
  const { figuresCard, figuresMeta, figuresMessage, figuresCopy, figuresRefresh } = dom;
  figuresCard.dataset.state = state;
  figuresMeta.textContent = meta;
  figuresMessage.textContent = message;
  figuresCopy.hidden = state !== "ready";
  figuresRefresh.disabled = state === "loading";
  figuresRefresh.querySelector("span")!.textContent = state === "error" ? "Try again" : "Refresh";
  if (state !== "ready") dom.figuresBody.replaceChildren();
  figuresCard.hidden = false;
}

export function hideFigures(): void {
  dom.figuresCard.hidden = true;
}

function row({ label, value }: Figure): HTMLLIElement {
  const li = create("li", "figures-row");
  li.append(create("span", "figures-label", label), create("strong", "figures-value", value));
  return li;
}

function renderFigures(figures: KeyFigures): void {
  const lines: string[] = [];
  const sections = GROUPS.filter(({ key }) => figures[key].length > 0).map(({ key, title }) => {
    lines.push(title, ...figures[key].map((f) => `- ${f.label}: ${f.value}`), "");
    const section = create("section", "figures-group");
    const list = create("ul", "figures-list");
    list.append(...figures[key].map(row));
    section.append(create("h4", "figures-group-title", title), list);
    return section;
  });
  copyText = lines.join("\n").trim();
  dom.figuresBody.replaceChildren(...sections);
}

async function run(refresh: boolean): Promise<void> {
  const id = session.viewedId;
  if (id === null) return;

  hideRecap();
  render("loading", "Reading the conversation…");
  try {
    const figures = await keyFigures(id, refresh, loadStoredApiKey());
    if (session.viewedId !== id) return;
    if (!figures) return render("empty", "", NOTHING_SAID);
    if (!GROUPS.some(({ key }) => figures[key].length > 0)) return render("empty", "", NONE_FOUND);
    render("ready", `as of ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`);
    renderFigures(figures);
  } catch (err) {
    if (session.viewedId === id) render("error", "", String(err));
  }
}

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(copyText);
    showToast("Key figures copied");
  } catch {
    showToast("Couldn't copy key figures");
  }
}

export function initFigures(): void {
  dom.assistFiguresItem.addEventListener("click", () => {
    closeAssistMenu();
    void run(false);
  });
  // One assist card at a time.
  dom.assistRecapItem.addEventListener("click", hideFigures);
  dom.figuresRefresh.addEventListener("click", () => void run(true));
  dom.figuresCopy.addEventListener("click", () => void copy());
  dom.figuresClose.addEventListener("click", hideFigures);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !dom.figuresCard.hidden) hideFigures();
  });
}
