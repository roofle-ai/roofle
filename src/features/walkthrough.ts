// "How it works": slides shown once after setup, reopenable from Settings → About.
// A slide with a `target` glows that element and puts the popup beside it with an arrow.
const SEEN_KEY = "roofle-walkthrough-seen";

interface Slide {
  title: string;
  body: string;
  target?: string;
  /** Hidden-until-needed element to show while the slide is up (e.g. Assist only exists once a call is selected). */
  reveal?: string;
}

const SLIDES: Slide[] = [
  {
    target: "#new-session-btn",
    title: "Start a conversation",
    body: "Click <b>New conversation</b>, use the menu-bar icon, or press <b>⌘R</b>. Roofle also notices Zoom and Teams calls. Pick the type of call and, if you like, a goal. That shapes the notes and the questions.",
  },
  {
    target: "#transcript-panel",
    title: "Follow along live",
    body: "The transcript covers your mic and the other side of the call. <b>Notes</b> update every few minutes, so you can stay in the conversation instead of typing.",
  },
  {
    target: "#questions-panel",
    title: "Questions worth asking",
    body: "While you talk, Roofle spots gaps, assumptions and open points, and suggests <b>follow-up questions</b> in the right panel, guided by your goal. Questions answered later move to <b>Answered</b>, and stale ones fade away. Click a question's source to jump to that moment, press <b>×</b> to dismiss one, or switch suggestions off.",
  },
  {
    target: "#assist-btn",
    reveal: "#assist-control",
    title: "Ask Assist",
    body: "Open <b>Assist</b> for one-click help: recap the last 30 seconds, pull out key figures and dates, or get a conversation review. It can also suggest <b>what to ask next</b>, say something simpler, show what the other side cares about, or confirm understanding.",
  },
  {
    title: "Wrap up",
    body: "Hit stop for a <b>recap</b>, key figures and a review. <b>Export</b> the transcript or notes, and search every past call in <b>History</b>. Audio never leaves your Mac, and AI runs on your own key.",
  },
  {
    target: "#settings-btn",
    title: "Make it yours",
    body: "In <b>Settings</b>, add your <b>AI key</b>, tell Roofle about your role in <b>Profile</b> so notes and questions fit you, tune how often notes update under <b>Live Session</b>, and check your AI <b>Usage</b>. You can reopen this guide from <b>About</b>.",
  },
];

document.body.insertAdjacentHTML(
  "beforeend",
  `<div id="walkthrough" class="analyse-overlay" role="dialog" aria-modal="true" aria-labelledby="walkthrough-title" hidden>
    <div id="walkthrough-dialog" class="analyse-dialog license-dialog">
      <div class="license-hero">
        <span id="walkthrough-step" class="license-badge"></span>
        <h2 id="walkthrough-title" class="license-headline"></h2>
        <p id="walkthrough-body" class="license-sub"></p>
      </div>
      <div class="license-body walkthrough-nav">
        <button id="walkthrough-back" type="button" class="settings-link">Back</button>
        <button id="walkthrough-next" type="button" class="btn-primary"></button>
      </div>
    </div>
    <span id="walkthrough-arrow" class="walkthrough-arrow" aria-hidden="true">➜</span>
  </div>`,
);

const root = document.getElementById("walkthrough")!;
const dialog = document.getElementById("walkthrough-dialog")!;
const arrow = document.getElementById("walkthrough-arrow")!;
const el = (id: string) => document.getElementById(id)!;
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

const MARGIN = 16;
const ARROW = 32;
const GAP = 12;
let index = 0;
let waiters: (() => void)[] = [];
let glowing: Element | null = null;
let revealed: { node: HTMLElement; wasHidden: boolean } | null = null;

function unpoint(): void {
  glowing?.classList.remove("tour-highlight");
  glowing = null;
  if (revealed) revealed.node.hidden = revealed.wasHidden;
  revealed = null;
  root.classList.remove("is-pointing");
}

/** Put the popup and arrow on the roomier side of `target`, vertically centred on it. */
function placeBeside(target: Element): void {
  const r = target.getBoundingClientRect();
  const width = Math.min(480, window.innerWidth - 2 * MARGIN);
  const need = width + 2 * GAP + ARROW;
  if (Math.max(r.left, window.innerWidth - r.right) - MARGIN < need) return placeInside(r);
  const targetOnLeft = r.left + r.width / 2 < window.innerWidth / 2;
  const left = targetOnLeft ? r.right + GAP + ARROW + GAP : r.left - GAP - ARROW - GAP - width;
  dialog.style.width = `${width}px`;
  dialog.style.left = `${clamp(left, MARGIN, window.innerWidth - width - MARGIN)}px`;
  const centre = r.top + r.height / 2;
  dialog.style.top = `${clamp(centre - dialog.offsetHeight / 2, MARGIN, window.innerHeight - dialog.offsetHeight - MARGIN)}px`;
  arrow.style.left = `${targetOnLeft ? r.right + GAP : r.left - GAP - ARROW}px`;
  arrow.style.top = `${clamp(centre - ARROW / 2, MARGIN, window.innerHeight - ARROW - MARGIN)}px`;
  root.dataset.side = targetOnLeft ? "left" : "right";
}

/** Target too wide to sit beside: popup over its lower part, arrow above it pointing up at the glowing panel. */
function placeInside(r: DOMRect): void {
  const width = Math.min(480, Math.max(320, r.width - 2 * MARGIN));
  const left = clamp(r.left + (r.width - width) / 2, MARGIN, window.innerWidth - width - MARGIN);
  dialog.style.width = `${width}px`;
  dialog.style.left = `${left}px`;
  const top = Math.max(MARGIN + ARROW + GAP, r.bottom - dialog.offsetHeight - MARGIN);
  dialog.style.top = `${top}px`;
  arrow.style.left = `${left + width / 2 - ARROW / 2}px`;
  arrow.style.top = `${top - GAP - ARROW}px`;
  root.dataset.side = "up";
}

function draw(): void {
  const s = SLIDES[index];
  el("walkthrough-step").textContent = `${index + 1} of ${SLIDES.length}`;
  el("walkthrough-title").textContent = s.title;
  el("walkthrough-body").innerHTML = s.body;
  el("walkthrough-back").style.visibility = index ? "visible" : "hidden";
  el("walkthrough-next").textContent = index === SLIDES.length - 1 ? "Got it" : "Next";
  unpoint();
  const target = s.target ? document.querySelector(s.target) : null;
  if (!target) return;
  const hidden = s.reveal ? document.querySelector<HTMLElement>(s.reveal) : null;
  if (hidden) {
    revealed = { node: hidden, wasHidden: hidden.hidden !== false };
    hidden.hidden = false;
  }
  target.classList.add("tour-highlight");
  glowing = target;
  root.classList.add("is-pointing");
  placeBeside(target);
}

function close(): void {
  root.hidden = true;
  unpoint();
  for (const done of waiters.splice(0)) done();
  try {
    localStorage.setItem(SEEN_KEY, "1");
  } catch {
    /* shows again next launch; harmless */
  }
}

el("walkthrough-back").addEventListener("click", () => {
  index -= 1;
  draw();
});
el("walkthrough-next").addEventListener("click", () => {
  if (index === SLIDES.length - 1) return close();
  index += 1;
  draw();
});
root.addEventListener("click", (e) => e.target === root && close());
document.addEventListener("keydown", (e) => e.key === "Escape" && !root.hidden && close());
window.addEventListener("resize", () => {
  if (!root.hidden) draw();
});

export function showWalkthrough(): void {
  index = 0;
  root.hidden = false; // visible first so the popup can be measured
  draw();
}

function seen(): boolean {
  try {
    return localStorage.getItem(SEEN_KEY) === "1";
  } catch {
    return false;
  }
}

export function showWalkthroughOnce(): void {
  if (!seen() && root.hidden) showWalkthrough();
}

/** Resolves once the guide is out of the way: at once if it was seen, else after showing it and the user closing it. */
export function walkthroughDone(): Promise<void> {
  if (seen() && root.hidden) return Promise.resolve();
  if (root.hidden) showWalkthrough();
  return new Promise((resolve) => waiters.push(resolve));
}
