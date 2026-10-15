import { openUrl } from "@tauri-apps/plugin-opener";

import { loadStoredApiKey } from "../ai";
import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { showToast } from "../lib/status";
import { getActionItems, getRecipe, runRecipe } from "../transcripts";
import type { ActionItem, Draft, RecipeKind, WrapUp } from "../types";

type Chip = RecipeKind | "action-items";

/** Give up waiting for the final notes (e.g. nothing was said, so no pass ran). */
const PENDING_TIMEOUT_MS = 120_000;
/** Sessions whose end-of-call notes are still being written. */
const pending = new Set<number>();
/** The session the result panel belongs to. */
let resultFor: number | null = null;
/** What the result panel currently holds, for Copy / Open in Mail. */
let current: { chip: Chip; draft?: Draft; items?: ActionItem[] } | null = null;

const hasNotes = (): boolean => dom.notesInput.value.trim() !== "";

/** A call just ended: recipes wait on the final notes. */
export function markWrapUpPending(id: number): void {
  pending.add(id);
  window.setTimeout(() => {
    if (pending.delete(id)) syncWrapUp();
  }, PENDING_TIMEOUT_MS);
  syncWrapUp();
}

/** The backend finished the end-of-call pass; the final notes are in the editor. */
export function onWrapUp({ session_id }: WrapUp): void {
  pending.delete(session_id);
  if (session.viewedId === session_id) syncWrapUp();
}

/**
 * The pill is offered for any finished session that
 * has notes; chips wait while the end-of-call notes are still being written.
 */
export function syncWrapUp(): void {
  const id = session.viewedId;
  const idle = session.state === "idle" && id !== null;
  const waiting = id !== null && pending.has(id);
  const available = idle && hasNotes();
  dom.wrapupPill.hidden = !(idle && (waiting || available));
  dom.wrapupTriggers.forEach((t) => (t.disabled = !available || waiting));
  if (!idle || resultFor !== id) closeResult();
}

function closeResult(): void {
  current = null;
  dom.wrapupResult.hidden = true;
}

const NAMES: Record<Chip, string> = {
  "recap-email": "Recap email",
  "slack-post": "Slack post",
  "action-items": "Action items",
};

type ResultState = "loading" | "ready";

function showResult(chip: Chip, state: ResultState, meta = ""): void {
  dom.wrapupResult.dataset.state = state;
  dom.wrapupTitle.textContent = NAMES[chip];
  dom.wrapupMeta.textContent = meta;
  dom.wrapupResult.hidden = false;
}

function showDraft(chip: RecipeKind, draft: Draft): void {
  current = { chip, draft };
  showResult(chip, "ready", draft.subject ? `Subject: ${draft.subject}` : "");
  dom.wrapupDraft.value = draft.body;
  dom.wrapupDraft.hidden = false;
  dom.wrapupItems.hidden = true;
  dom.wrapupMail.hidden = chip !== "recap-email";
  dom.wrapupRefresh.hidden = dom.wrapupCopy.hidden = false;
}

function showItems(items: ActionItem[]): void {
  current = { chip: "action-items", items };
  showResult("action-items", "ready");
  dom.wrapupItems.replaceChildren(...(items.length ? items.map(itemRow) : [create("li", "wrapup-empty", "No action items were captured.")]));
  dom.wrapupItems.hidden = false;
  dom.wrapupDraft.hidden = true;
  dom.wrapupMail.hidden = dom.wrapupRefresh.hidden = true;
  dom.wrapupCopy.hidden = false;
}

function itemRow(item: ActionItem): HTMLLIElement {
  const row = create("li", "wrapup-item");
  row.append(
    create("span", "wrapup-task", item.task),
    create("span", item.owner ? "wrapup-tag" : "wrapup-tag is-missing", item.owner ?? "No owner"),
    create("span", item.due ? "wrapup-tag" : "wrapup-tag is-missing", item.due ?? "No date"),
  );
  return row;
}

/** Open a recipe: a stored draft is shown as-is; the LLM runs only when none exists or on `refresh`. */
async function runChip(chip: Chip, refresh = false): Promise<void> {
  const id = session.viewedId;
  if (id === null) return;
  resultFor = id;
  showResult(chip, "loading");
  dom.wrapupDraft.hidden = dom.wrapupItems.hidden = dom.wrapupMail.hidden = dom.wrapupRefresh.hidden = dom.wrapupCopy.hidden = true;
  try {
    if (chip === "action-items") {
      const items = await getActionItems(id);
      if (session.viewedId === id) showItems(items);
    } else {
      const draft = (!refresh && (await getRecipe(id, chip))) || (await runRecipe(id, chip, loadStoredApiKey()));
      if (session.viewedId === id) showDraft(chip, draft);
    }
  } catch (err) {
    if (session.viewedId === id) {
      closeResult();
      showToast(String(err));
    }
  }
}

/** The text Copy puts on the clipboard: the edited draft, or a markdown checklist. */
function copyText(): string {
  if (current?.items) {
    return current.items
      .map((i) => `- [ ] ${i.owner ? `${i.owner}: ` : ""}${i.task}${i.due ? ` — by ${i.due}` : ""}`)
      .join("\n");
  }
  return dom.wrapupDraft.value;
}

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(copyText());
    showToast(`${current ? NAMES[current.chip] : "Text"} copied`);
  } catch {
    showToast("Couldn't copy");
  }
}

function openInMail(): void {
  const subject = encodeURIComponent(current?.draft?.subject ?? "");
  const body = encodeURIComponent(dom.wrapupDraft.value);
  openUrl(`mailto:?subject=${subject}&body=${body}`).catch(() => showToast("Couldn't open Mail"));
}

export function initWrapUp(): void {
  dom.wrapupTriggers.forEach((t) =>
    t.addEventListener("click", () => void runChip(t.dataset.recipe as Chip)),
  );
  dom.notesInput.addEventListener("input", syncWrapUp);
  dom.wrapupCopy.addEventListener("click", () => void copy());
  dom.wrapupMail.addEventListener("click", openInMail);
  dom.wrapupRefresh.addEventListener("click", () => {
    if (current && current.chip !== "action-items") void runChip(current.chip, true);
  });
  dom.wrapupClose.addEventListener("click", closeResult);
}
