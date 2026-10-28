// First-run checklist: license, speech model, permissions, AI key. Every row reads live state,
// so it also catches a permission granted in System Settings. Hidden for good once done or dismissed.
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

import { loadAiConfig } from "../ai";
import { showLicenseModal } from "../license";
import { openSettings } from "../settings";
import { showWalkthroughOnce } from "./walkthrough";

const DONE_KEY = "roofle-onboarding-done";
const POLL_MS = 2000;
const PANE = "x-apple.systempreferences:com.apple.preference.security?Privacy_";

interface Step {
  id: string;
  title: string;
  hint: string;
  done: boolean;
  action?: { label: string; run: () => void };
}

const CHECK = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>`;

const card = document.getElementById("onboarding")!;
let timer: number | undefined;
let lastRender = "";

function isDone(): boolean {
  try {
    return localStorage.getItem(DONE_KEY) === "1";
  } catch {
    return false;
  }
}

function markDone(): void {
  try {
    localStorage.setItem(DONE_KEY, "1");
  } catch {
    /* shows again next launch; harmless */
  }
}

async function steps(): Promise<Step[]> {
  const [lic, modelReady, perms, ai] = await Promise.all([
    invoke<{ required: boolean; licensed: boolean }>("license_status"),
    invoke<boolean>("models_ready"),
    invoke<{ mic: string; screen: boolean }>("permissions_status"),
    loadAiConfig(),
  ]);
  const list: Step[] = [];
  if (lic.required) {
    list.push({
      id: "license",
      title: "Activate your beta license",
      hint: "Enter the key from your invite.",
      done: lic.licensed,
      action: { label: "Enter key", run: showLicenseModal },
    });
  }
  list.push({
    id: "model",
    title: "Speech model",
    hint: modelReady ? "Ready." : "Downloading automatically. This takes a few minutes, once.",
    done: modelReady,
  });
  list.push({
    id: "mic",
    title: "Microphone access",
    hint: "So Roofle can hear you.",
    done: perms.mic === "granted",
    action:
      perms.mic === "undetermined"
        ? { label: "Allow", run: () => void invoke("request_permission", { kind: "mic" }) }
        : { label: "Open Settings", run: () => void openUrl(`${PANE}Microphone`) },
  });
  list.push({
    id: "screen",
    title: "Screen & system audio recording",
    hint: "So Roofle can hear the other side of the call. Restart Roofle after enabling.",
    done: perms.screen,
    action: {
      label: "Allow",
      run: () => {
        void invoke("request_permission", { kind: "screen" });
        void openUrl(`${PANE}ScreenCapture`);
      },
    },
  });
  list.push({
    id: "ai",
    title: "AI key",
    hint: "Powers live notes, questions and recaps. Bring your own key.",
    done: ai.has_api_key,
    action: { label: "Open Settings", run: openSettings },
  });
  return list;
}

function render(list: Step[]): void {
  const left = list.filter((s) => !s.done).length;
  const html = `
    <div class="onboarding-head">
      <strong>Get set up: ${left} step${left > 1 ? "s" : ""} left</strong>
      <button type="button" class="onboarding-skip" data-skip>Dismiss</button>
    </div>
    <ul class="onboarding-list">
      ${list
        .map(
          (s) => `<li class="onboarding-step${s.done ? " is-done" : ""}">
            <span class="onboarding-tick">${s.done ? CHECK : ""}</span>
            <span class="onboarding-text"><b>${s.title}</b><span>${s.hint}</span></span>
            ${!s.done && s.action ? `<button type="button" class="model-banner-btn onboarding-btn" data-step="${s.id}">${s.action.label}</button>` : ""}
          </li>`,
        )
        .join("")}
    </ul>`;
  if (html === lastRender) return; // keep the buttons stable while polling
  lastRender = html;
  card.innerHTML = html;
  card.querySelector("[data-skip]")!.addEventListener("click", hide);
  for (const btn of card.querySelectorAll<HTMLButtonElement>("[data-step]")) {
    btn.addEventListener("click", () => list.find((s) => s.id === btn.dataset.step)?.action?.run());
  }
}

function hide(): void {
  markDone();
  clearInterval(timer);
  card.hidden = true;
}

async function refresh(): Promise<void> {
  const list = await steps();
  if (list.every((s) => s.done)) {
    hide(); // finished: never shown again
    showWalkthroughOnce(); // the guide comes right after setup completes
    return;
  }
  render(list);
}

export function initOnboarding(): void {
  if (isDone()) {
    showWalkthroughOnce(); // onboarding finished or dismissed earlier; no-op once seen
    return;
  }
  card.hidden = false;
  void refresh();
  timer = window.setInterval(() => void refresh(), POLL_MS);
}
