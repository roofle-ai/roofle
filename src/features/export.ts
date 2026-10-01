import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";

import { session } from "../app/state";
import { dom } from "../lib/dom";
import { exportStamp } from "../lib/format";
import { showToast } from "../lib/status";
import { getSession } from "../transcripts";

type ExportKind = "transcript" | "notes";

function closeMenu(): void {
  dom.assistMenu.hidden = true;
  dom.assistBtn.setAttribute("aria-expanded", "false");
}

async function contentFor(kind: ExportKind): Promise<string> {
  if (kind === "notes") return dom.notesInput.value;
  if (session.viewedId === null) return "";
  try {
    return (await getSession(session.viewedId))?.transcript ?? "";
  } catch {
    return "";
  }
}

/** Write a `.txt` to the app exports folder and reveal it in the file manager. */
async function runExport(kind: ExportKind): Promise<void> {
  closeMenu();
  const content = await contentFor(kind);
  if (!content.trim()) {
    showToast("Nothing to export yet");
    return;
  }

  const label = kind === "transcript" ? "Transcript" : "Notes";
  const filename = `Roofle-${label}-${exportStamp()}.txt`;
  try {
    const path = await invoke<string>("export_text", { filename, content });
    showToast(`Exported: ${path}`);
    openPath(path.slice(0, path.lastIndexOf("/"))).catch(() => {});
  } catch (err) {
    showToast(String(err));
  }
}

/** Export lives in the Assist menu; wire its two export items. */
export function initExport(): void {
  dom.assistMenu.querySelectorAll<HTMLButtonElement>("[data-export]").forEach((item) => {
    item.addEventListener("click", () => void runExport(item.dataset.export as ExportKind));
  });
}
