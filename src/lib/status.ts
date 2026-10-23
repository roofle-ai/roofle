import { session } from "../app/state";
import { dom } from "./dom";

const TOAST_MS = 2200;
let toastTimer: number | undefined;

export type StatusKind = "idle" | "recording" | "paused" | "busy" | "error";

const IDLE_TEXT = { idle: "Idle", recording: "Recording", paused: "Paused" } as const;

/** Turn a raw backend error into something a user can act on; the raw text stays in the tooltip. */
function friendlyError(raw: string): string {
  const l = raw.toLowerCase();
  const what = l.includes("notes generation") ? "Couldn't generate notes" : "Something went wrong";
  if (l.includes("timed out") || l.includes("timeout")) return `${what}: the AI took too long to respond. Try again.`;
  if (l.includes("401") || l.includes("403") || l.includes("api key") || l.includes("unauthorized"))
    return `${what}: check your AI settings and API key.`;
  if (l.includes("429") || l.includes("rate limit")) return `${what}: too many requests. Wait a moment and retry.`;
  if (l.includes("connect") || l.includes("network") || l.includes("dns") || l.includes("offline"))
    return `${what}: can't reach the AI service. Check your connection.`;
  if (l.includes("parse")) return `${what}: the AI sent an unreadable reply. Try again.`;
  return l.includes("notes generation") ? `${what}. Try again.` : raw;
}

export function setStatus(kind: StatusKind, text: string): void {
  const isError = kind === "error";
  dom.statusDot.className = `status-dot ${kind}`;
  dom.statusText.textContent = isError ? friendlyError(text) : text;
  dom.statusText.title = isError ? text : "";
  dom.statusDismiss.hidden = !isError;
  const pillState = kind === "busy" ? "paused" : kind;
  dom.sessionPill.className = `session-pill ${pillState}`;
}

/** Hide an error: fall back to the state of the current session. */
export function dismissStatus(): void {
  setStatus(session.state, IDLE_TEXT[session.state]);
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
