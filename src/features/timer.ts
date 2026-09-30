import { session } from "../app/state";
import { dom } from "../lib/dom";
import { formatTime } from "../lib/format";

let interval: number | undefined;

export function startTimer(): void {
  // `recording-state` and the transport button can both start the timer; a
  // leftover interval would make the clock run fast.
  stopTimer();
  dom.timer.textContent = formatTime(session.elapsedSec);
  interval = window.setInterval(() => {
    session.elapsedSec += 1;
    dom.timer.textContent = formatTime(session.elapsedSec);
  }, 1000);
}

export function stopTimer(): void {
  window.clearInterval(interval);
  interval = undefined;
}

export function resetTimer(): void {
  session.elapsedSec = 0;
  dom.timer.textContent = formatTime(0);
}
