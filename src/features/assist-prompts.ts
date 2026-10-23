// Assist quick prompts: categorised, one-click questions the LLM will answer from the transcript.
// UI only for now — `runAssistPrompt` is where the LLM call plugs in.
import { create, dom } from "../lib/dom";
import { refreshIcons } from "../lib/icons";
import { showToast } from "../lib/status";
import { closeAssistMenu } from "./analysis";

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

/** Add categories or prompts here; the menu renders from this list. */
const CATEGORIES: AssistCategory[] = [
  {
    id: "respond-well",
    title: "Respond well",
    accent: "green",
    prompts: [
      {
        id: "next-question",
        icon: "message-circle-question",
        label: "What should I ask next?",
        description: "Three follow-up questions based on the last minute.",
      },
      {
        id: "answer-this",
        icon: "reply",
        label: "Help me answer this",
        description: "Drafts a short reply to the question just asked of you.",
      },
      {
        id: "handle-objection",
        icon: "shield-check",
        label: "Handle this objection",
        description: "Two ways to respond to the concern they just raised.",
      },
      {
        id: "say-simpler",
        icon: "lightbulb",
        label: "Say it simpler",
        description: "Plain-language version of what was just explained.",
      },
    ],
  },
  {
    id: "understand-position",
    title: "Understand their position",
    accent: "insight",
    prompts: [
      {
        id: "probe-interests",
        icon: "target",
        label: "What do they care about?",
        description: "Questions to probe their interests and motivations.",
      },
      {
        id: "uncover-constraints",
        icon: "lock",
        label: "What limits them?",
        description: "Questions to uncover budget, timing and approval constraints.",
      },
      {
        id: "assess-decision-criteria",
        icon: "scale",
        label: "How will they decide?",
        description: "Questions to assess their decision criteria and process.",
      },
      {
        id: "unsaid",
        icon: "eye-off",
        label: "What haven't they said?",
        description: "Topics they avoided or left vague.",
      },
    ],
  },
  {
    id: "stay-on-track",
    title: "Stay on track",
    accent: "amber",
    prompts: [
      {
        id: "on-goal",
        icon: "flag",
        label: "Are we on goal?",
        description: "Checks the talk against the goal you set at call start.",
      },
      {
        id: "confirm-understanding",
        icon: "handshake",
        label: "Confirm understanding",
        description: "A short recap to read back to them and check you're aligned.",
      },
    ],
  },
];

/** Run a prompt against the transcript. Placeholder until the LLM call is wired in. */
function runAssistPrompt(prompt: AssistPrompt): void {
  showToast(`${prompt.label} is coming soon`);
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
    runAssistPrompt(prompt);
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
  refreshIcons();
}
