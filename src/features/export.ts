import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { session } from "../app/state";
import { dom } from "../lib/dom";
import { exportStamp } from "../lib/format";
import { showToast } from "../lib/status";
import { getSession } from "../transcripts";
import { closeMenu } from "./topics";

type ExportKind = "transcript" | "notes";

async function contentFor(kind: ExportKind): Promise<string> {
  if (kind === "notes") return dom.notesInput.value;
  if (session.viewedId === null) return "";
  try {
    return (await getSession(session.viewedId))?.transcript ?? "";
  } catch {
    return "";
  }
}

/** Write a `.txt` to ~/Downloads and reveal it in the file manager. */
export async function runExport(kind: ExportKind): Promise<void> {
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
    revealItemInDir(path).catch(() => {});
  } catch (err) {
    showToast(String(err));
  }
}
