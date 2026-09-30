import { session } from "../app/state";
import { create, dom } from "../lib/dom";
import { formatDuration, formatWhen, previewWords } from "../lib/format";
import { refreshIcons } from "../lib/icons";
import { setStatus } from "../lib/status";
import {
  deleteSession,
  getSession,
  listSessionQuestions,
  listSessions,
  type SessionSummary,
} from "../transcripts";
import { updateAnalyseVisibility } from "./analysis";
import { refreshNotesTitle } from "./notes";
import { ingestChallenges, resetQuestions, setAnalysisMode } from "./questions";
import { resetView } from "./view";

export async function loadHistory(): Promise<void> {
  const sessions = await listSessions();
  dom.historyList.querySelectorAll(".history-item").forEach((el) => el.remove());
  dom.historyCount.textContent = String(sessions.length);
  dom.historyEmpty.hidden = sessions.length !== 0;
  dom.historyList.append(...sessions.map(renderItem));
  refreshIcons();
}

function renderItem(s: SessionSummary): HTMLElement {
  const item = create("div", "history-item");
  item.dataset.id = String(s.id);
  item.classList.toggle("is-active", s.id === session.viewedId);
  item.classList.toggle("is-recording", s.status === "recording");

  const title = create("span", "history-title", s.title || s.preview || "(untitled)");
  const meta = create(
    "span",
    "history-meta",
    `${formatDuration(s.duration_secs)} · ${formatWhen(s.started_at)}`,
  );
  item.append(title, meta);

  item.tabIndex = 0;
  item.setAttribute("role", "button");
  item.setAttribute("aria-label", `Open session ${title.textContent}`);
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

function renderDeleteButton(id: number): HTMLButtonElement {
  const del = create("button", "history-delete");
  del.type = "button";
  del.title = "Delete session";
  del.setAttribute("aria-label", "Delete session");
  del.innerHTML = `<i data-lucide="trash" aria-hidden="true"></i>`;
  del.addEventListener("click", (e) => {
    e.stopPropagation();
    void deleteSession(id)
      .then(() => {
        if (session.viewedId === id) {
          session.viewedId = null;
          refreshNotesTitle();
          resetView();
          updateAnalyseVisibility();
        }
        void loadHistory();
      })
      .catch((err) => setStatus("error", String(err)));
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

    resetQuestions();
    setAnalysisMode(true);
    try {
      ingestChallenges(await listSessionQuestions(id), 0);
    } catch {
      // Live updates will populate the panel if the stored list can't load.
    }
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
