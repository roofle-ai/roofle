import { dom } from "./dom";

const TOAST_MS = 2200;
let toastTimer: number | undefined;

export type StatusKind = "idle" | "recording" | "paused" | "busy" | "error";

export function setStatus(kind: StatusKind, text: string): void {
  dom.statusDot.className = `status-dot ${kind}`;
  dom.statusText.textContent = text;
  const pillState = kind === "busy" ? "paused" : kind;
  dom.sessionPill.className = `session-pill ${pillState}`;
}

export function showToast(msg: string): void {
  dom.toast.textContent = msg;
  dom.toast.classList.add("show");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => dom.toast.classList.remove("show"), TOAST_MS);
}

export function describeError(err: unknown): string {
  const raw = String(err);
  const lower = raw.toLowerCase();
  if (lower.includes("screen recording") || lower.includes("screenrecording")) {
    return "Screen Recording permission needed: System Settings → Privacy & Security → Screen Recording, enable Roofle, then restart.";
  }
  if (lower.includes("microphone") || lower.includes("mic")) {
    return "Microphone permission needed: System Settings → Privacy & Security → Microphone, enable Roofle.";
  }
  if (lower.includes("model") && lower.includes("download")) {
    return `Model issue: ${raw}`;
  }
  return raw;
}
