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
