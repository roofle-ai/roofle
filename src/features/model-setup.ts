import { invoke } from "@tauri-apps/api/core";

import { dom } from "../lib/dom";
import { describeError, setStatus } from "../lib/status";

/** True when the speech model is on disk; otherwise shows the download banner. */
export async function ensureModel(): Promise<boolean> {
  const ready = await invoke<boolean>("models_ready");
  setReady(ready);
  return ready;
}

/** Banner and "New conversation" are mirror images of model availability. */
function setReady(ready: boolean): void {
  dom.modelSetup.hidden = ready;
  dom.newSessionBtn.disabled = !ready;
  dom.newSessionBtn.title = ready ? "" : "Speech model is still downloading";
}

const INTRO = dom.modelSetupText.textContent ?? "";
let downloading = false;

function hideProgress(): void {
  dom.modelProgress.hidden = true;
  dom.modelProgressFill.style.width = "0%";
}

async function download(): Promise<void> {
  downloading = true;
  dom.modelDownloadBtn.hidden = true;
  dom.modelSetupText.textContent = INTRO;
  setStatus("busy", "Preparing model…");
  try {
    await invoke("download_model");
    hideProgress();
    setReady(true);
    setStatus("idle", "Idle");
  } catch (err) {
    hideProgress();
    dom.modelSetupText.textContent = `${describeError(err)}. Check your connection and retry.`;
    dom.modelDownloadBtn.textContent = "Retry";
    dom.modelDownloadBtn.hidden = false;
    setStatus("idle", "Idle");
  } finally {
    downloading = false;
  }
}

export function initModelSetup(): void {
  dom.modelDownloadBtn.addEventListener("click", () => {
    if (!downloading) void download();
  });
  // Start the one-time download on first launch; the banner shows progress, Retry appears only on failure.
  void ensureModel().then((ready) => {
    if (!ready) void download();
  });
}
