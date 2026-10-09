import { dom } from "../lib/dom";

export interface CallSetup {
  /** A `MeetingType` value, e.g. decision, review, sales or other. */
  kind: string;
  goal: string;
}

/** Example goals offered as one-click chips, per call type. */
const GOAL_IDEAS: Record<string, string[]> = {
  decision: ["Decide between two options", "Agree on a go/no-go"],
  review: ["Find the risks in the plan", "Agree what to change before build"],
  sales: ["Qualify budget and timeline", "Agree the next step"],
  status: ["Find blockers and owners", "Confirm the delivery date"],
  interview: ["Judge fit for the role", "Probe real depth of experience"],
  discovery: ["Pin down scope and constraints", "Find who decides and when"],
  negotiation: ["Agree price and terms", "Find what is flexible"],
  planning: ["Agree priorities and owners", "Settle the first milestone"],
  training: ["Check what has been understood", "Capture open questions"],
  support: ["Find the root cause", "Agree the fix and its owner"],
  other: [],
};

/** Ask what the call is about; resolves null when the user cancels. */
export function askCallSetup(): Promise<CallSetup | null> {
  return new Promise((resolve) => {
    const done = (value: CallSetup | null) => {
      dom.callSetupModal.hidden = true;
      dom.callSetupStart.removeEventListener("click", onStart);
      dom.callSetupCancel.removeEventListener("click", onCancel);
      dom.callSetupClose.removeEventListener("click", onCancel);
      dom.callSetupGoal.removeEventListener("keydown", onKey);
      dom.callSetupGoal.removeEventListener("input", syncCount);
      dom.callSetupType.removeEventListener("change", renderChips);
      resolve(value);
    };
    const syncCount = () => (dom.callSetupCount.textContent = `${dom.callSetupGoal.value.length}/${dom.callSetupGoal.maxLength}`);
    const renderChips = () => {
      dom.callSetupChips.replaceChildren(
        ...(GOAL_IDEAS[dom.callSetupType.value] ?? []).map((idea) => {
          const b = document.createElement("button");
          b.type = "button";
          b.className = "analyse-chip";
          b.textContent = idea;
          b.onclick = () => {
            dom.callSetupGoal.value = idea;
            syncCount();
            dom.callSetupGoal.focus();
          };
          return b;
        }),
      );
    };
    const onStart = () => done({ kind: dom.callSetupType.value, goal: dom.callSetupGoal.value.trim() });
    const onCancel = () => done(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onStart();
      else if (e.key === "Escape") onCancel();
    };

    dom.callSetupStart.addEventListener("click", onStart);
    dom.callSetupCancel.addEventListener("click", onCancel);
    dom.callSetupClose.addEventListener("click", onCancel);
    dom.callSetupGoal.addEventListener("keydown", onKey);
    dom.callSetupGoal.addEventListener("input", syncCount);
    dom.callSetupType.addEventListener("change", renderChips);
    dom.callSetupGoal.value = "";
    syncCount();
    renderChips();
    dom.callSetupModal.hidden = false;
    dom.callSetupGoal.focus();
  });
}
