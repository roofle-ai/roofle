import { initEvents } from "./app/events";
import { initLayout } from "./app/layout";
import { initTheme } from "./app/theme";
import { initAnalysis } from "./features/analysis";
import { initExport } from "./features/export";
import { loadHistory } from "./features/history";
import { initRecap } from "./features/recap";
import { initNotes } from "./features/notes";
import { initNotesTabs } from "./features/notes-tabs";
import { initTransport } from "./features/transport";
import { refreshIcons } from "./lib/icons";
import { openSettings } from "./settings";

async function init(): Promise<void> {
  refreshIcons();
  initTheme();
  initLayout();
  initTransport(openSettings);
  initNotes();
  initNotesTabs();
  initRecap();
  initAnalysis();
  initExport();

  await loadHistory();
  await initEvents();
}

void init();
