import { dom } from "../lib/dom";

export interface CallSetup {
  /** A `MeetingType` value, e.g. decision, review, sales or other. */
  kind: string;
  goal: string;
}

/** Per dropdown value: the backend `MeetingType` it maps to, and goal chips. */
const TYPES: Record<string, { kind: string; ideas: string[] }> = {
  decision: { kind: "decision", ideas: ["Decide between the options on the table", "Agree a clear go or no-go"] },
  recruitment: { kind: "interview", ideas: ["Agree the hiring criteria and timeline", "Align on the candidate shortlist"] },
  agile: { kind: "planning", ideas: ["Surface blockers and who owns them", "Agree the sprint scope and priorities"] },
  software: { kind: "review", ideas: ["Find the biggest risks in the design", "Agree the technical approach and owners"] },
  "taking-interview": { kind: "interview", ideas: ["Judge fit for the role", "Test the depth of their experience"] },
  "giving-interview": { kind: "interview", ideas: ["Show my strengths for the role", "Learn what the team and role really involve"] },
  "sales-selling": { kind: "sales", ideas: ["Qualify their budget and timeline", "Agree a concrete next step"] },
  "sales-buying": { kind: "sales", ideas: ["Check the product meets our requirements", "Understand pricing, terms and risks"] },
  negotiation: { kind: "negotiation", ideas: ["Agree price and terms", "Learn what each side can flex on"] },
  planning: { kind: "planning", ideas: ["Agree priorities and owners", "Settle the first milestone"] },
  training: { kind: "training", ideas: ["Check what has been understood", "Capture open questions"] },
  lecture: { kind: "training", ideas: ["Capture the key takeaways", "Note questions to follow up"] },
  other: { kind: "other", ideas: ["Agree the outcome and next steps", "Capture decisions and action items"] },
};

/** Ask what the call is about; resolves null when the user cancels. */
export function askCallSetup(): Promise<CallSetup | null> {
  return new Promise((resolve) => {
    const done = (value: CallSetup | null) => {
      dom.callSetupModal.hidden = true;
      dom.callSetupStart.removeEventListener("click", onStart);
      dom.callSetupCancel.removeEventListener("click", onCancel);
      dom.callSetupClose.removeEventListener("click", onCancel);
      dom.callSetupModal.removeEventListener("keydown", onKey);
      dom.callSetupModal.removeEventListener("mousedown", onBackdrop);
      dom.callSetupGoal.removeEventListener("input", syncCount);
      dom.callSetupType.removeEventListener("change", renderChips);
      resolve(value);
    };
    const syncCount = () => (dom.callSetupCount.textContent = `${dom.callSetupGoal.value.length}/${dom.callSetupGoal.maxLength}`);
    const renderChips = () => {
      dom.callSetupChips.replaceChildren(
        ...(TYPES[dom.callSetupType.value]?.ideas ?? []).map((idea) => {
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
    const onStart = () => done({ kind: TYPES[dom.callSetupType.value]?.kind ?? "other", goal: dom.callSetupGoal.value.trim() });
    const onCancel = () => done(null);
    const onBackdrop = (e: MouseEvent) => {
      if (e.target === dom.callSetupModal) onCancel();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onStart();
      else if (e.key === "Escape") onCancel();
    };

    dom.callSetupStart.addEventListener("click", onStart);
    dom.callSetupCancel.addEventListener("click", onCancel);
    dom.callSetupClose.addEventListener("click", onCancel);
    dom.callSetupModal.addEventListener("keydown", onKey);
    dom.callSetupModal.addEventListener("mousedown", onBackdrop);
    dom.callSetupGoal.addEventListener("input", syncCount);
    dom.callSetupType.addEventListener("change", renderChips);
    dom.callSetupGoal.value = "";
    syncCount();
    renderChips();
    dom.callSetupModal.hidden = false;
    dom.callSetupGoal.focus();
  });
}
