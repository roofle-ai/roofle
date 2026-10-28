import { loadStoredApiKey } from "../ai";
import { session } from "../app/state";
import { dom } from "../lib/dom";
import { showToast } from "../lib/status";
import { recapSession } from "../transcripts";
import { hideAssistCard } from "./assist-prompts";
import { closeAssistMenu } from "./analysis";
import { hideFigures } from "./figures";

const RECAP_WINDOW_SECS = 30;
const DEFAULT_TITLE = `Last ${RECAP_WINDOW_SECS} seconds`;
const NOTHING_SAID = "Nothing has been said yet. Try again once the conversation gets going.";

type RecapState = "loading" | "ready" | "empty" | "error";

interface RecapView {
  state: RecapState;
  title: string;
  meta?: string;
  body?: string;
}

function renderRecap({ state, title, meta = "", body = "" }: RecapView): void {
  const { recapCard, recapTitle, recapMeta, recapBody, recapCopy, recapRefresh } = dom;
  recapCard.dataset.state = state;
  recapTitle.textContent = title;
  recapMeta.textContent = meta;
  recapBody.textContent = body;
  recapCopy.hidden = state !== "ready";
  recapRefresh.disabled = state === "loading";
  recapRefresh.querySelector("span")!.textContent = state === "error" ? "Try again" : "Refresh";
  recapCard.hidden = false;
}

export function hideRecap(): void {
  dom.recapCard.hidden = true;
}

/** "Last 30 seconds", or "So far" when the conversation is younger than the window. */
function recapTitle(coveredSecs: number): string {
  return coveredSecs >= RECAP_WINDOW_SECS ? DEFAULT_TITLE : "So far";
}

function recapMeta(coveredSecs: number): string {
  const asOf = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return coveredSecs >= RECAP_WINDOW_SECS ? `as of ${asOf}` : `${coveredSecs}s of conversation · as of ${asOf}`;
}

async function runRecap(): Promise<void> {
  const id = session.viewedId;
  if (id === null) return;

  hideAssistCard();
  hideFigures();
  renderRecap({ state: "loading", title: DEFAULT_TITLE, meta: "Catching up…" });
  try {
    const recap = await recapSession(id, RECAP_WINDOW_SECS, loadStoredApiKey());
    if (session.viewedId !== id) return;
    if (!recap) return renderRecap({ state: "empty", title: "Recap", body: NOTHING_SAID });
    renderRecap({
      state: "ready",
      title: recapTitle(recap.covered_secs),
      meta: recapMeta(recap.covered_secs),
      body: recap.summary,
    });
  } catch (err) {
    if (session.viewedId === id) {
      renderRecap({ state: "error", title: "Recap unavailable", body: String(err) });
    }
  }
}

async function copyRecap(): Promise<void> {
  try {
    await navigator.clipboard.writeText(dom.recapBody.textContent ?? "");
    showToast("Recap copied");
  } catch {
    showToast("Couldn't copy the recap");
  }
}

export function initRecap(): void {
  dom.assistRecapItem.addEventListener("click", () => {
    closeAssistMenu();
    void runRecap();
  });
  dom.recapRefresh.addEventListener("click", () => void runRecap());
  dom.recapCopy.addEventListener("click", () => void copyRecap());
  dom.recapClose.addEventListener("click", hideRecap);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !dom.recapCard.hidden) hideRecap();
  });
}
