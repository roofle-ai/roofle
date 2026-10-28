// Assist quick prompts: categorised, one-click questions the LLM will answer from the transcript.
// Each prompt id has a matching system prompt in `live-agent/prompts/assist/`.
import { loadStoredApiKey } from "../ai";
import { session } from "../app/state";
import data from "../data/assist-prompts.json";
import { create, dom } from "../lib/dom";
import { refreshIcons } from "../lib/icons";
import { showToast } from "../lib/status";
import { assistPrompt } from "../transcripts";
import { closeAssistMenu } from "./analysis";
import { hideFigures } from "./figures";
import { hideRecap } from "./recap";

interface AssistPrompt {
  id: string;
  /** Lucide icon name. */
  icon: string;
  label: string;
  /** One line on what the prompt does. */
  description: string;
}

interface AssistCategory {
  id: string;
  title: string;
  /** Colour family (a `--<accent>` design token) that tints the category's card. */
  accent: "brand" | "insight" | "green" | "amber" | "record";
  prompts: AssistPrompt[];
}

/** Add categories or prompts to `src/data/assist-prompts.json`; the menu renders from this list. */
const CATEGORIES: AssistCategory[] = data.categories as AssistCategory[];

const NOTHING_SAID = "Nothing has been said yet. Try again once the conversation gets going.";

type CardState = "loading" | "ready" | "empty" | "error";

/** The prompt behind the card, so Refresh can run it again. */
let current: AssistPrompt | null = null;

function renderCard(state: CardState, title: string, meta = "", body = ""): void {
  const { assistCard, assistCardTitle, assistCardMeta, assistCardBody, assistCardCopy, assistCardRefresh } = dom;
  assistCard.dataset.state = state;
  assistCardTitle.textContent = title;
  assistCardMeta.textContent = meta;
  assistCardBody.textContent = body;
  assistCardCopy.hidden = state !== "ready";
  assistCardRefresh.disabled = state === "loading";
  assistCardRefresh.querySelector("span")!.textContent = state === "error" ? "Try again" : "Refresh";
  assistCard.hidden = false;
}

/** Run a prompt against the transcript and show the answer in the card. */
async function runAssistPrompt(prompt: AssistPrompt): Promise<void> {
  const id = session.viewedId;
  if (id === null) return;
  current = prompt;
  hideRecap();
  hideFigures();

  renderCard("loading", prompt.label, "Thinking…");
  try {
    const answer = await assistPrompt(id, prompt.id, loadStoredApiKey());
    if (session.viewedId !== id || current !== prompt) return;
    if (!answer) return renderCard("empty", prompt.label, "", NOTHING_SAID);
    const asOf = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    renderCard("ready", prompt.label, `as of ${asOf}`, answer);
  } catch (err) {
    if (session.viewedId === id && current === prompt) renderCard("error", "Assist unavailable", "", String(err));
  }
}

function renderPrompt(prompt: AssistPrompt): HTMLButtonElement {
  const button = create("button");
  button.type = "button";
  button.setAttribute("role", "menuitem");
  button.dataset.prompt = prompt.id;
  button.innerHTML = `<i data-lucide="${prompt.icon}" aria-hidden="true"></i>
    <span class="assist-item-text">
      <span class="assist-item-label"></span>
      <span class="assist-item-desc"></span>
    </span>`;
  button.querySelector(".assist-item-label")!.textContent = prompt.label;
  button.querySelector(".assist-item-desc")!.textContent = prompt.description;
  button.addEventListener("click", () => {
    closeAssistMenu();
    void runAssistPrompt(prompt);
  });
  return button;
}

function renderCategory({ id, title, accent, prompts }: AssistCategory): HTMLElement {
  const group = create("div", "assist-group");
  group.dataset.accent = accent;
  group.setAttribute("role", "group");
  group.setAttribute("aria-labelledby", `assist-${id}`);
  const heading = create("div", "assist-group-title", title);
  heading.id = `assist-${id}`;
  group.append(heading, ...prompts.map(renderPrompt));
  return group;
}

export function initAssistPrompts(): void {
  dom.assistPrompts.replaceChildren(...CATEGORIES.map(renderCategory));
  dom.assistCardRefresh.addEventListener("click", () => current && void runAssistPrompt(current));
  dom.assistCardCopy.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(dom.assistCardBody.textContent ?? "");
      showToast("Copied");
    } catch {
      showToast("Couldn't copy");
    }
  });
  dom.assistCardClose.addEventListener("click", hideAssistCard);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !dom.assistCard.hidden) hideAssistCard();
  });
  refreshIcons();
}

export function hideAssistCard(): void {
  dom.assistCard.hidden = true;
  current = null;
}
