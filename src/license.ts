// License gate popup. The backend is the real gate; this only collects the key and explains why start was refused.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { showToast } from "./lib/status";

const CHECK = `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>`;
const PRICE = import.meta.env.VITE_LICENSE_PRICE; // optional, e.g. "$20"
const LIST_PRICE = import.meta.env.VITE_LICENSE_LIST_PRICE; // optional, e.g. "$40"
const BENEFITS = [
  "<b>Pay once, own it for life.</b> No subscription, ever.",
  "<b>Every future update included</b>, new features too.",
  "<b>Your data stays yours.</b> Transcription runs on your Mac, and you bring your own AI key.",
  "Use it on up to <b>2 of your devices</b>.",
];

/** Benefits, buy button and key field. Shared by the popup and onboarding; `onActivated` runs after a valid key. */
export function mountLicensePanel(root: HTMLElement, onActivated: () => void): void {
  root.innerHTML = `
    <form class="license-form">
      <ul class="license-benefits">${BENEFITS.map((b) => `<li>${CHECK}<span>${b}</span></li>`).join("")}</ul>
      <div class="license-price" ${PRICE ? "" : "hidden"}>
        <span class="license-price-now">${PRICE ?? ""}</span>
        <span class="license-price-was" ${LIST_PRICE ? "" : "hidden"}>${LIST_PRICE ?? ""}</span>
        <span class="license-price-note">one-time payment, lifetime access</span>
      </div>
      <button type="button" class="license-cta">Get my lifetime license, 50% off</button>
      <div class="license-divider"><span>Already purchased?</span></div>
      <div class="license-redeem">
        <input class="analyse-input" name="key" placeholder="Paste your license key" aria-label="License key" autocomplete="off" spellcheck="false" required />
        <button type="submit" class="btn-primary">Activate</button>
      </div>
      <p class="analyse-desc license-error" role="alert"></p>
    </form>`;
  const form = root.querySelector<HTMLFormElement>("form")!;
  const key = form.elements.namedItem("key") as HTMLInputElement;
  const submit = form.querySelector<HTMLButtonElement>("[type=submit]")!;
  const error = form.querySelector(".license-error")!;

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    submit.disabled = true;
    error.textContent = "";
    try {
      await invoke("activate_license", { key: key.value });
      showToast("License activated. You can start a conversation now.");
      onActivated();
    } catch (err) {
      error.textContent = String(err);
    } finally {
      submit.disabled = false;
    }
  });
  form.querySelector(".license-cta")!.addEventListener("click", () => {
    const url = import.meta.env.VITE_LICENSE_BUY_URL;
    if (url) void openUrl(url);
  });
}

/** Whether the gate is on and this install is not yet licensed. */
export async function needsLicense(): Promise<boolean> {
  const { required, licensed } = await invoke<{ required: boolean; licensed: boolean }>("license_status");
  return required && !licensed;
}

document.body.insertAdjacentHTML(
  "beforeend",
  `<div id="license-modal" class="analyse-overlay" role="dialog" aria-modal="true" aria-labelledby="license-title" hidden>
    <div class="analyse-dialog license-dialog">
      <button id="license-close" type="button" class="analyse-close license-close" aria-label="Close">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
      </button>
      <div class="license-hero">
        <span class="license-badge">50% OFF · Launch offer</span>
        <h2 id="license-title" class="license-headline">Never miss a moment of your calls.</h2>
        <p class="license-sub">Live transcripts, smart notes and follow-up questions while you talk. Unlock Roofle with a one-time license.</p>
      </div>
      <div id="license-mount" class="license-body"></div>
    </div>
  </div>`,
);

const modal = document.getElementById("license-modal")!;
const close = () => (modal.hidden = true);

export function showLicenseModal(): void {
  mountLicensePanel(document.getElementById("license-mount")!, close); // fresh form each time
  modal.hidden = false;
  modal.querySelector<HTMLInputElement>("input")!.focus();
}

/** True when the user may start; otherwise opens the buy/activate popup. */
export async function ensureLicensed(): Promise<boolean> {
  if (!(await needsLicense())) return true;
  showLicenseModal();
  return false;
}

document.getElementById("license-close")!.addEventListener("click", close);
modal.addEventListener("click", (e) => e.target === modal && close());
document.addEventListener("keydown", (e) => e.key === "Escape" && close());
void listen("license-required", showLicenseModal); // tray "New conversation" while unlicensed
