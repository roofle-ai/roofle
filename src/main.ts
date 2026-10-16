import { initEvents } from "./app/events";
import { initLayout } from "./app/layout";
import { initTheme } from "./app/theme";
import { initAnalysis } from "./features/analysis";
import { initExport } from "./features/export";
import { loadHistory } from "./features/history";
import { initFigures } from "./features/figures";
import { initRecap } from "./features/recap";
import { initModelSetup } from "./features/model-setup";
import { initNotes } from "./features/notes";
import { initNotesTabs } from "./features/notes-tabs";
import { initTransport } from "./features/transport";
import { initWrapUp } from "./features/wrap-up";
import { refreshIcons } from "./lib/icons";
import { openSettings } from "./settings";
import "./auth";

async function init(): Promise<void> {
  refreshIcons();
  initTheme();
  initLayout();
  initTransport(openSettings);
  initNotes();
  initNotesTabs();
  initRecap();
  initFigures();
  initAnalysis();
  initExport();
  initWrapUp();
  initModelSetup();

  await loadHistory();
  await initEvents();
}

void init();
