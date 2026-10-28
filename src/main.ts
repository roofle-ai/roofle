import "@fontsource-variable/inter/wght.css";
import "@fontsource-variable/newsreader/wght.css";
import "@fontsource-variable/newsreader/wght-italic.css";
import { initEvents } from "./app/events";
import { initLayout } from "./app/layout";
import { initTheme } from "./app/theme";
import { initAnalysis } from "./features/analysis";
import { initAssistPrompts } from "./features/assist-prompts";
import { loadHistory } from "./features/history";
import { initHistorySearch } from "./features/history-search";
import { initFigures } from "./features/figures";
import { initRecap } from "./features/recap";
import { initModelSetup } from "./features/model-setup";
import { initOnboarding } from "./features/onboarding";
import { initNotes } from "./features/notes";
import { initNotesTabs } from "./features/notes-tabs";
import { initTopics } from "./features/topics";
import { initTransport } from "./features/transport";
import { initWrapUp } from "./features/wrap-up";
import { openUrl } from "@tauri-apps/plugin-opener";
import { refreshIcons } from "./lib/icons";
import { dismissStatus, showToast } from "./lib/status";
import { dom } from "./lib/dom";
import { openSettings } from "./settings";
import { initPersona } from "./persona";

async function init(): Promise<void> {
  refreshIcons();
  initTheme();
  initLayout();
  dom.statusDismiss.addEventListener("click", dismissStatus);
  document.getElementById("feedback-btn")!.addEventListener("click", () => {
    const url = import.meta.env.VITE_FEEDBACK_URL;
    if (url) void openUrl(url);
    else showToast("The feedback form isn't set up yet.");
  });
  initTransport(openSettings);
  initNotes();
  initNotesTabs();
  initRecap();
  initFigures();
  initAnalysis();
  initAssistPrompts();
  initWrapUp();
  initModelSetup();
  initOnboarding();

  initHistorySearch(() => void loadHistory());
  await initTopics();
  await loadHistory();
  await initEvents();
  await initPersona();
}

void init();
