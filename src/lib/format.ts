const TITLE_WORDS = 10;

export function formatTime(totalSec: number): string {
  const total = Math.floor(totalSec);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function previewWords(text: string, max: number = TITLE_WORDS): string {
  return text.split(/\s+/).filter(Boolean).slice(0, max).join(" ");
}

/** `next_step_strength` -> "Next Step Strength". */
export function humanizeKey(k: string): string {
  return k.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

export function formatScore(v: number): string {
  return Number.isInteger(v) ? String(v) : v.toFixed(1);
}

/** `YYYYMMDD-HHMMSS` in local time. */
export function exportStamp(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const date = `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}`;
  return `${date}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}
