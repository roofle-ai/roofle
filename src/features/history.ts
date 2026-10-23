import { session } from "../app/state";
import { confirmDialog } from "../lib/confirm";
import { create, dom } from "../lib/dom";
import { previewWords } from "../lib/format";
import { refreshIcons } from "../lib/icons";
import { setStatus } from "../lib/status";
import {
  deleteSession,
  getSession,
  listSessionQuestions,
  listSessions,
  sessionTitle,
  type SessionSummary,
} from "../transcripts";
import { updateAnalyseVisibility } from "./analysis";
import { refreshNotesTitle, showLoadedNotes } from "./notes";
import { ingestChallenges, resetQuestions, setAnalysisMode, updateFollowupToggle } from "./questions";
import { matchesSearch } from "./history-search";
import { groupByTopic, rememberTopics, topicName } from "./topics";
import { resetView } from "./view";
import { syncWrapUp } from "./wrap-up";

export async function loadHistory(): Promise<void> {
  const sessions = await listSessions();
  rememberTopics(sessions);
  dom.historyList.querySelectorAll(".history-item, .topic-bucket").forEach((el) => el.remove());
  dom.historyRailBadge.hidden = sessions.length === 0;
  dom.historyRailBadge.textContent = sessions.length > 99 ? "99+" : String(sessions.length);
  dom.historyEmpty.hidden = sessions.length !== 0;
  // A conversation is found by its title or by the topic it is filed under.
  const visible = sessions.filter((s) => matchesSearch(sessionTitle(s)) || matchesSearch(topicName(s.topic_id) ?? ""));
  const rows = groupByTopic(visible, renderItem);
  dom.historyNoResults.hidden = rows.length !== 0 || sessions.length === 0;
  dom.historyList.append(...rows);
  refreshIcons();
  syncTransportPlacement();
}

function renderItem(s: SessionSummary): HTMLElement {
  const item = create("div", "history-item");
  item.dataset.id = String(s.id);
  item.classList.toggle("is-active", s.id === session.viewedId);
  item.classList.toggle("is-recording", s.status === "recording");

  const title = create("span", "history-title", sessionTitle(s));
  item.append(title);
  item.title = title.textContent ?? "";

  item.tabIndex = 0;
  item.setAttribute("role", "button");
  item.setAttribute("aria-label", `Open conversation ${title.textContent}`);
  item.addEventListener("click", (e) => {
    if ((e.target as HTMLElement).closest(".history-delete")) return;
    void openSession(s.id);
  });
  item.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      void openSession(s.id);
    }
  });

  item.append(renderDeleteButton(s.id));
  return item;
}

const ROW_PAUSE = `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>`;
const ROW_PLAY = `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5.14v13.72c0 .9.98 1.45 1.75.98l10.2-6.86a1.15 1.15 0 0 0 0-1.96L9.75 4.16A1.15 1.15 0 0 0 8 5.14z"/></svg>`;
const ROW_STOP = `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>`;

function rowButton(label: string, icon: string, target: HTMLElement, cls = ""): HTMLButtonElement {
  const b = create("button", cls);
  b.type = "button";
  b.title = b.ariaLabel = label;
  b.innerHTML = icon;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    target.click(); // reuse the main controls' handlers
  });
  return b;
}

/**
 * The transport lives in the notes header while the live conversation is open; when
 * another one is open it moves to the live conversation's row in this list.
 */
export function syncTransportPlacement(): void {
  const live = session.state !== "idle" && session.activeId !== null;
  dom.floatingControl.classList.toggle("is-idle", !live || session.activeId !== session.viewedId);
  dom.historyList.querySelectorAll(".row-transport").forEach((el) => el.remove());
  if (!live) return;
  const item = dom.historyList.querySelector<HTMLElement>(`.history-item[data-id="${session.activeId}"]`);
  if (!item) return;
  const box = create("span", "row-transport");
  const paused = session.state === "paused";
  box.append(
    rowButton(paused ? "Resume recording" : "Pause recording", paused ? ROW_PLAY : ROW_PAUSE, dom.recordBtn, paused ? "paused" : "recording"),
    rowButton("Stop conversation", ROW_STOP, dom.stopBtn),
  );
  item.append(box);
}

/** Delete a conversation after confirming, resetting the notes view if it was the one open. */
export async function removeSession(id: number): Promise<void> {
  const ok = await confirmDialog(
    "Delete this conversation?",
    "Its notes and transcript will be permanently deleted. This can't be undone.",
    "Delete",
  );
  if (!ok) return;
  try {
    await deleteSession(id);
    if (session.viewedId === id) {
      session.viewedId = null;
      refreshNotesTitle();
      resetView();
      updateAnalyseVisibility();
      syncWrapUp();
    }
    await loadHistory();
  } catch (err) {
    setStatus("error", String(err));
  }
}

function renderDeleteButton(id: number): HTMLButtonElement {
  const del = create("button", "history-delete");
  del.type = "button";
  del.title = "Delete conversation";
  del.setAttribute("aria-label", "Delete conversation");
  del.innerHTML = `<i data-lucide="trash" aria-hidden="true"></i>`;
  del.addEventListener("click", (e) => {
    e.stopPropagation();
    void removeSession(id);
  });
  return del;
}

export async function openSession(id: number): Promise<void> {
  try {
    const detail = await getSession(id);
    if (!detail) return;

    session.viewedId = id;
    updateAnalyseVisibility();

    resetView();
    // Prefer the persisted title (agent-renamed) over the transcript's first words.
    session.titleFromTranscript =
      detail.session.title?.trim() ||
      (detail.transcript?.trim() ? previewWords(detail.transcript) : undefined);
    refreshNotesTitle(new Date(detail.session.started_at));
    session.hasTranscript = detail.segments.length > 0;
    dom.notesInput.value = detail.notes ?? "";
    showLoadedNotes();
    syncWrapUp();

    resetQuestions();
    setAnalysisMode(true);
    try {
      ingestChallenges(await listSessionQuestions(id));
    } catch {
      // Live updates will populate the panel if the stored list can't load.
    }
    updateFollowupToggle();
    await loadHistory();
  } catch (err) {
    setStatus("error", String(err));
  }
}

/** A fresh recording has no known id; adopt the newest recording session. */
export async function adoptActiveSession(): Promise<void> {
  if (session.viewedId !== null) return;
  try {
    const sessions = await listSessions();
    const target = sessions.find((s) => s.status === "recording") ?? sessions[0];
    if (target) session.viewedId = target.id;
  } catch {
    // Autosave waits until a session id is known.
  }
}
