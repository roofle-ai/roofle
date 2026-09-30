import { initEvents } from "./app/events";
import { initLayout } from "./app/layout";
import { initTheme } from "./app/theme";
import { initAnalysis } from "./features/analysis";
import { initExport } from "./features/export";
import { loadHistory } from "./features/history";
import { initNotes } from "./features/notes";
import { initTransport } from "./features/transport";
import { refreshIcons } from "./lib/icons";
import { openSettings } from "./settings";

async function init(): Promise<void> {
  refreshIcons();
  initTheme();
  initLayout();
  initTransport(openSettings);
  initNotes();
  initAnalysis();
  initExport();

  await loadHistory();
  await initEvents();
}

void init();
