// Topics: user-created buckets. A finished conversation is filed under at most
// one topic from the ⋯ menu in the notes header; the sidebar groups
// conversations by topic, and unfiled ones sit outside any bucket. Assignment is
// manual — nothing is inferred.
import { session } from "../app/state";
import { confirmDialog } from "../lib/confirm";
import { create, dom } from "../lib/dom";
import { refreshIcons } from "../lib/icons";
import { setStatus } from "../lib/status";
import { createTopic, deleteTopic, listTopics, renameSession, setSessionTopic, type SessionSummary, type Topic } from "../transcripts";
import { runExport } from "./export";
import { loadHistory, removeSession } from "./history";
import { isSearching, matchesSearch } from "./history-search";
import { isEditingNotes, refreshNotesTitle, startEditingNotes } from "./notes";

let topics: Topic[] = [];
const topicOf = new Map<number, number | null>();
const collapsed = new Set<number>();

export const topicName = (id: number | null): string | undefined => topics.find((t) => t.id === id)?.name;

const fail = (err: unknown) => setStatus("error", String(err));

/** History loaded: remember each session's topic for the menu. */
export function rememberTopics(sessions: SessionSummary[]): void {
  topicOf.clear();
  sessions.forEach((s) => topicOf.set(s.id, s.topic_id));
  syncTopic();
}

/**
 * Unfiled rows (newest first) above the collapsible topic buckets. Empty buckets are listed,
 * except while searching, when only buckets whose name or conversations match show, expanded.
 */
export function groupByTopic(
  sessions: SessionSummary[],
  renderItem: (s: SessionSummary) => HTMLElement,
): HTMLElement[] {
  const buckets = topics.flatMap((t) => {
    const mine = sessions.filter((s) => s.topic_id === t.id);
    const searching = isSearching();
    if (searching && mine.length === 0 && !matchesSearch(t.name)) return [];
    const open = searching || !collapsed.has(t.id);
    const bucket = create("div", "topic-bucket");
    bucket.classList.toggle("is-open", open);
    // Golden-angle spacing keeps neighbouring ids visibly different; stable across reloads.
    bucket.style.setProperty("--topic-hue", String((t.id * 137.5) % 360));

    const head = create("button", "topic-bucket-head");
    head.type = "button";
    head.setAttribute("aria-expanded", String(open));
    head.innerHTML = `<i data-lucide="chevron-right" class="topic-chevron" aria-hidden="true"></i><i data-lucide="${open ? "folder-open" : "folder"}" aria-hidden="true"></i>`;
    head.append(create("span", "topic-bucket-name", t.name), create("span", "topic-bucket-count", String(mine.length)));
    head.addEventListener("click", () => {
      collapsed.has(t.id) ? collapsed.delete(t.id) : collapsed.add(t.id);
      void loadHistory();
    });

    const del = create("button", "topic-bucket-delete");
    del.type = "button";
    del.title = "Delete topic (conversations are kept)";
    del.setAttribute("aria-label", `Delete topic ${t.name}`);
    del.innerHTML = `<i data-lucide="trash" aria-hidden="true"></i>`;
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      void removeTopic(t);
    });

    const row = create("div", "topic-bucket-row");
    row.append(head, del);
    bucket.append(row);
    bucket.classList.toggle("has-active", !open && mine.some((s) => s.id === session.viewedId));
    if (open) {
      const items = create("div", "topic-bucket-items");
      items.append(...(mine.length ? mine.map(renderItem) : [create("p", "topic-bucket-empty", "No conversations yet")]));
      bucket.append(items);
    }
    return [bucket];
  });
  const known = new Set(topics.map((t) => t.id));
  const unfiled = sessions.filter((s) => s.topic_id === null || !known.has(s.topic_id));
  return [...unfiled.map(renderItem), ...buckets];
}

/** Delete a topic after confirming; its conversations are kept, unfiled. */
async function removeTopic(t: Topic): Promise<void> {
  const ok = await confirmDialog(
    "Delete this topic?",
    `"${t.name}" will be deleted. Its conversations are kept and move out of the topic.`,
    "Delete",
  );
  if (!ok) return;
  try {
    await deleteTopic(t.id);
    await refreshTopics();
    await loadHistory();
  } catch (err) {
    fail(err);
  }
}

async function refreshTopics(): Promise<void> {
  topics = await listTopics();
}

async function assign(topicId: number | null): Promise<void> {
  const id = session.viewedId;
  if (id === null) return;
  try {
    await setSessionTopic(id, topicId);
    closeMenu();
    await loadHistory();
  } catch (err) {
    fail(err);
  }
}

/** Create a topic from typed text and refresh the sidebar; resolves to the new topic. */
async function addTopic(name: string): Promise<Topic | null> {
  name = name.trim();
  if (!name) return null;
  try {
    const t = await createTopic(name);
    await refreshTopics();
    return t;
  } catch (err) {
    fail(err);
    return null;
  }
}

/** Inline name field; Enter submits, Escape or blur cancels. */
function nameInput(onSubmit: (name: string) => Promise<void>, onCancel: () => void): HTMLInputElement {
  const input = create("input", "topic-new");
  input.type = "text";
  input.maxLength = 60;
  input.placeholder = "Topic name, then Enter";
  input.setAttribute("aria-label", "New topic name");
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") onCancel();
    if (e.key === "Enter" && input.value.trim()) void onSubmit(input.value);
  });
  input.addEventListener("blur", () => window.setTimeout(onCancel, 120));
  return input;
}

export function closeMenu(): void {
  dom.topicMenu.hidden = true;
  dom.topicBtn.setAttribute("aria-expanded", "false");
}

/** Which page of the ⋯ menu is showing: the option list, or the topic picker. */
let view: "root" | "topic" = "root";

function item(label: string, icon: string, onClick: () => void): HTMLButtonElement {
  const b = create("button", "topic-menu-item");
  b.type = "button";
  b.setAttribute("role", "menuitem");
  b.innerHTML = `<i data-lucide="${icon}" aria-hidden="true"></i>`;
  b.append(create("span", "", label));
  b.addEventListener("click", onClick);
  return b;
}

function go(next: "root" | "topic"): void {
  view = next;
  renderMenu();
}

function renderMenu(): void {
  const current = session.viewedId === null ? null : (topicOf.get(session.viewedId) ?? null);

  if (view === "root") {
    // Options live here; add new ones as more `item(...)` rows.
    const topic = item(`Topic: ${topicName(current) ?? "None"}`, "folder", () => go("topic"));
    topic.insertAdjacentHTML("beforeend", `<i data-lucide="chevron-right" aria-hidden="true"></i>`);
    const busy = session.state !== "idle";
    topic.disabled = busy;
    const editNotes = item("Edit notes", "pencil", () => {
      closeMenu();
      startEditingNotes();
    });
    editNotes.disabled = busy || session.notesTab !== "notes" || isEditingNotes();
    const rename = item("Rename conversation", "pencil", () => {
      const input = nameInput(
        async (title) => {
          const id = session.viewedId;
          if (id === null) return;
          try {
            await renameSession(id, title.trim());
            session.titleFromTranscript = title.trim();
            refreshNotesTitle();
            closeMenu();
            await loadHistory();
          } catch (err) {
            fail(err);
          }
        },
        renderMenu,
      );
      input.value = session.titleFromTranscript ?? "";
      input.placeholder = "Title, then Enter";
      input.setAttribute("aria-label", "Conversation title");
      rename.replaceWith(input);
      input.focus();
      input.select();
    });
    rename.disabled = busy;
    const del = item("Delete conversation", "trash", () => {
      closeMenu();
      if (session.viewedId !== null) void removeSession(session.viewedId);
    });
    del.classList.add("topic-menu-danger");
    del.disabled = busy;
    dom.topicMenu.replaceChildren(
      editNotes,
      rename,
      topic,
      create("div", "topic-menu-sep"),
      item("Export transcript", "download", () => void runExport("transcript")),
      item("Export notes", "download", () => void runExport("notes")),
      create("div", "topic-menu-sep"),
      del,
    );
    return refreshIcons();
  }

  const row = (label: string, id: number | null, icon: string) => {
    const b = item(label, icon, () => void assign(id));
    b.setAttribute("role", "menuitemradio");
    b.setAttribute("aria-checked", String(id === current));
    if (id === current) b.insertAdjacentHTML("beforeend", `<i data-lucide="check" class="topic-check" aria-hidden="true"></i>`);
    return b;
  };
  const add = item("New topic…", "folder-plus", () => {
    const input = nameInput(
      async (name) => {
        const t = await addTopic(name);
        if (t) await assign(t.id);
      },
      renderMenu,
    );
    add.replaceWith(input);
    input.focus();
  });
  add.classList.add("topic-menu-add");
  const back = item("Topic", "chevron-left", () => go("root"));
  back.classList.add("topic-menu-back");
  dom.topicMenu.replaceChildren(
    back,
    create("div", "topic-menu-sep"),
    row("No topic", null, "folder"),
    ...topics.map((t) => row(t.name, t.id, "tag")),
    create("div", "topic-menu-sep"),
    add,
  );
  refreshIcons();
}

/** The ⋯ menu is offered whenever a conversation is open in the notes header. */
export function syncTopic(): void {
  const show = session.viewedId !== null;
  dom.topicControl.hidden = !show;
  if (!show) closeMenu();
  else if (!dom.topicMenu.hidden) renderMenu();
}

export async function initTopics(): Promise<void> {
  await refreshTopics().catch(fail);

  dom.topicBtn.addEventListener("click", () => {
    const open = dom.topicMenu.hidden;
    if (open) go("root");
    dom.topicMenu.hidden = !open;
    dom.topicBtn.setAttribute("aria-expanded", String(open));
  });
  document.addEventListener("click", (e) => {
    if (!e.composedPath().includes(dom.topicControl)) closeMenu();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !dom.topicMenu.hidden) closeMenu();
  });
}
