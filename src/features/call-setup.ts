import { dom } from "../lib/dom";

export interface CallSetup {
  /** A `MeetingType` value: decision, review, sales, status, interview or other. */
  kind: string;
  goal: string;
}

/** Ask what the call is about; resolves null when the user cancels. */
export function askCallSetup(): Promise<CallSetup | null> {
  return new Promise((resolve) => {
    const done = (value: CallSetup | null) => {
      dom.callSetupModal.hidden = true;
      dom.callSetupStart.removeEventListener("click", onStart);
      dom.callSetupCancel.removeEventListener("click", onCancel);
      dom.callSetupClose.removeEventListener("click", onCancel);
      dom.callSetupGoal.removeEventListener("keydown", onKey);
      resolve(value);
    };
    const onStart = () => done({ kind: dom.callSetupType.value, goal: dom.callSetupGoal.value.trim() });
    const onCancel = () => done(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") onStart();
      else if (e.key === "Escape") onCancel();
    };

    dom.callSetupStart.addEventListener("click", onStart);
    dom.callSetupCancel.addEventListener("click", onCancel);
    dom.callSetupClose.addEventListener("click", onCancel);
    dom.callSetupGoal.addEventListener("keydown", onKey);
    dom.callSetupModal.hidden = false;
    dom.callSetupGoal.focus();
  });
}
