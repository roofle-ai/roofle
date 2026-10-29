// Persona: job role + meeting types. Chosen at first run, editable in Settings, stored in the app DB.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ensureModel } from "./features/model-setup";
import data from "./data/personas.json";
import { refreshIcons } from "./lib/icons";

export interface Persona {
  roleId: string | null;
  meetings: string[];
  /** Editable first-person summary, max SUMMARY_MAX chars. */
  summary?: string;
}

const SUMMARY_MAX = 400;

const roles = data.roles;
const EMPTY: Persona = { roleId: null, meetings: [] };

/** null until the user has answered or skipped. */
export const loadPersona = () => invoke<Persona | null>("get_persona");
export const savePersona = (persona: Persona) => invoke("save_persona", { persona });

/** Role cards; picking one reveals that role's meeting types as multi-select chips. */
export function mountPersonaPicker(
  root: HTMLElement,
  initial: Persona,
  onChange: (p: Persona) => void,
  show: "role" | "meetings" | "both" = "both",
): void {
  let p: Persona = { roleId: initial.roleId, meetings: [...initial.meetings] };

  const render = () => {
    const role = roles.find((r) => r.id === p.roleId);
    const heads = show === "both";
    const cards =
      show === "meetings"
        ? ""
        : `${heads ? '<p class="persona-q">What do you do?</p>' : ""}
           <div class="persona-cards">${roles
             .map(
               (r) => `<button type="button" class="persona-card" data-role="${r.id}" aria-pressed="${r.id === p.roleId}">
                 <span class="persona-icon"><i data-lucide="${r.icon}" aria-hidden="true"></i></span>
                 <span class="persona-label">${r.label}</span></button>`,
             )
             .join("")}</div>`;
    const chips =
      show === "role" || !role
        ? ""
        : `${heads ? '<p class="persona-q">Which meetings do you join?</p>' : ""}
           <div class="persona-chips">${role.meetings
             .map((m) => `<button type="button" class="persona-chip" data-type="${m.type}" aria-pressed="${p.meetings.includes(m.type)}">${m.label}</button>`)
             .join("")}</div>`;
    root.innerHTML = cards + chips;
    refreshIcons();
  };

  root.onclick = (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-role], [data-type]");
    if (!el) return;
    if (el.dataset.role) {
      if (el.dataset.role === p.roleId) return;
      p = { roleId: el.dataset.role, meetings: [] }; // meeting types belong to the role
    } else {
      const t = el.dataset.type!;
      p.meetings = p.meetings.includes(t) ? p.meetings.filter((x) => x !== t) : [...p.meetings, t];
    }
    render();
    onChange({ roleId: p.roleId, meetings: [...p.meetings] });
  };
  render();
}

type Step = "role" | "meetings" | "summary" | "model";

const COPY: Record<Step, { title: string; sub: string }> = {
  role: {
    title: "What best describes your role?",
    sub: "A sales call, a design review and an interview each need different things. Your role lets Roofle tailor its notes and follow-up questions to your work instead of giving generic ones. It takes 20 seconds, and you can change it any time in Settings.",
  },
  meetings: {
    title: "Which meetings do you join?",
    sub: "Tell us which meetings you join so Roofle knows what matters in them, such as next steps in a sales call or risks in a review. Pick all that apply.",
  },
  summary: {
    title: "Your persona",
    sub: "This is how Roofle will describe you to its AI. Edit it to say it your own way.",
  },
  model: {
    title: "Download the speech model",
    sub: "Roofle transcribes on your Mac, so your audio never leaves it. This one-time download is needed before your first conversation.",
  },
};

/** First run: role, meetings, then the required model download. Starts at the first unfinished part. */
export async function initPersona(): Promise<void> {
  const personaDone = (await loadPersona()) !== null;
  const modelReady = await invoke<boolean>("models_ready");
  const steps: Step[] = [];
  if (!personaDone) steps.push("role", "meetings", "summary");
  if (!modelReady) steps.push("model");
  if (steps.length === 0) return;

  document.body.insertAdjacentHTML(
    "beforeend",
    `<div id="persona-modal" class="onb" role="dialog" aria-modal="true" aria-labelledby="onb-title">
      <div class="onb-panel">
        <div class="onb-top">
          <span class="onb-brand">Make Roofle yours</span>
          <span id="onb-step" class="onb-step"></span>
        </div>
        <div id="onb-progress" class="onb-progress"></div>
        <h2 id="onb-title" class="onb-title"></h2>
        <p id="onb-sub" class="onb-sub"></p>
        <div id="persona-picker" class="onb-picker"></div>
        <p id="persona-error" class="onb-error" role="alert"></p>
        <div class="onb-actions">
          <button id="onb-left" type="button" class="btn-ghost"></button>
          <button id="onb-next" type="button" class="btn-primary onb-next"></button>
        </div>
      </div>
    </div>`,
  );
  const $ = (id: string) => document.getElementById(id)!;
  const next = $("onb-next") as HTMLButtonElement;
  const left = $("onb-left") as HTMLButtonElement;
  const error = $("persona-error");
  $("onb-progress").style.gridTemplateColumns = `repeat(${steps.length}, 1fr)`;
  $("onb-progress").innerHTML = "<i></i>".repeat(steps.length);
  let at = 0;
  let choice = EMPTY;
  let downloading = false;

  const finish = () => {
    $("persona-modal").remove();
    void ensureModel(); // sync the main window's banner and New conversation button
  };
  const advance = () => (at + 1 < steps.length ? go(at + 1) : finish());
  const savePersonaThen = async (p: Persona) => {
    try {
      await savePersona(p); // saved before later steps so quitting there does not re-ask
      advance();
    } catch (err) {
      error.textContent = String(err);
    }
  };

  const download = async () => {
    downloading = true;
    next.disabled = true;
    next.textContent = "Downloading…";
    error.textContent = "";
    const bar = $("onb-model-fill");
    const text = $("onb-model-text");
    const off = await listen<{ downloaded: number; total: number }>("model-progress", ({ payload: { downloaded, total } }) => {
      if (total <= 0) return;
      const pct = Math.round((downloaded / total) * 100);
      bar.style.width = `${pct}%`;
      text.textContent = `Downloading… ${pct}%`;
    });
    try {
      await invoke("download_model");
      advance();
    } catch (err) {
      error.textContent = `Download failed: ${String(err).replace(/^Error:\s*/, "")}. Check your connection and press Retry. It restarts from the beginning.`;
      next.disabled = false;
      next.textContent = "Retry";
    } finally {
      off();
      downloading = false;
    }
  };

  const render = () => {
    const step = steps[at];
    $("onb-step").textContent = `Step ${at + 1} of ${steps.length}`;
    $("onb-progress").querySelectorAll("i").forEach((bar, n) => bar.classList.toggle("on", n <= at));
    $("onb-title").textContent = COPY[step].title;
    $("onb-sub").textContent = COPY[step].sub;
    error.textContent = "";
    const picker = $("persona-picker");
    picker.classList.toggle("onb-card", step === "model");
    left.hidden = step === "model"; // the model download cannot be skipped
    if (step === "summary") {
      left.textContent = "Back";
      next.textContent = at + 1 < steps.length ? "Continue" : "Get started";
      next.disabled = false;
      picker.innerHTML = `<textarea id="onb-summary" class="onb-summary" maxlength="${SUMMARY_MAX}" rows="5" aria-label="Persona summary"></textarea>
        <span id="onb-summary-count" class="onb-count"></span>`;
      const box = $("onb-summary") as HTMLTextAreaElement;
      const count = () => ($("onb-summary-count").textContent = `${box.value.length} / ${SUMMARY_MAX}`);
      box.addEventListener("input", () => ((choice = { ...choice, summary: box.value }), count()));
      if (choice.summary !== undefined) (box.value = choice.summary), count();
      else
        invoke<string>("describe_persona", { persona: choice })
          .then((t) => ((box.value = choice.summary = t.slice(0, SUMMARY_MAX)), count()))
          .catch((err) => (error.textContent = String(err)));
    } else if (step === "role" || step === "meetings") {
      left.textContent = step === "role" ? "Skip for now" : "Back";
      next.textContent = at + 1 < steps.length ? "Continue" : "Get started";
      next.disabled = step === "role" && !choice.roleId;
      mountPersonaPicker(picker, choice, (p) => ((choice = { ...p, summary: undefined }), (next.disabled = !p.roleId)), step === "role" ? "role" : "meetings");
    } else {
      next.textContent = "Download model";
      next.disabled = false;
      picker.innerHTML = `
        <p class="onb-note"><b>Please keep Roofle open until it finishes.</b> If it's interrupted (you quit, or the connection drops), nothing is saved half-way. You'll see this step again, and the download restarts from the beginning. Roofle can't transcribe until it completes.</p>
        <div class="model-progress"><div class="model-progress-track" aria-hidden="true"><div id="onb-model-fill" class="model-progress-fill"></div></div>
        <span id="onb-model-text" class="model-progress-text"></span></div>`;
    }
  };
  const go = (n: number) => ((at = n), render());

  next.addEventListener("click", () => {
    const step = steps[at];
    if (step === "role") go(at + 1);
    else if (step === "meetings") go(at + 1);
    else if (step === "summary") void savePersonaThen(choice);
    else if (!downloading) void download();
  });
  left.addEventListener("click", () => {
    const step = steps[at];
    if (step === "role") void savePersonaThen(EMPTY); // skip: save the empty persona so it is not asked again
    else if (step === "meetings" || step === "summary") go(at - 1);
    else advance();
  });
  render();
}
