// Email OTP login/logout via Supabase. Session is persisted by supabase-js (localStorage).
import { createClient, type AuthError } from "@supabase/supabase-js";
import { refreshIcons } from "./lib/icons";
import { showToast } from "./lib/status";

const supabase = createClient(import.meta.env.VITE_SUPABASE_URL, import.meta.env.VITE_SUPABASE_ANON_KEY);

const btn = document.getElementById("auth-btn") as HTMLButtonElement;

document.body.insertAdjacentHTML(
  "beforeend",
  `<div id="auth-modal" class="analyse-overlay" role="dialog" aria-modal="true" aria-labelledby="auth-title" hidden>
    <form id="auth-form" class="analyse-dialog">
      <div class="analyse-head">
        <h2 id="auth-title" class="analyse-title">Log in</h2>
        <button id="auth-close" type="button" class="analyse-close" aria-label="Close">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </div>
      <div class="analyse-field">
        <label class="analyse-label" for="auth-email">Email</label>
        <span class="analyse-desc">We'll email you a one-time code.</span>
        <input id="auth-email" type="email" class="analyse-input analyse-textarea" required autocomplete="email" />
      </div>
      <div id="auth-code-field" class="analyse-field" hidden>
        <label class="analyse-label" for="auth-code">One-time code</label>
        <span class="analyse-desc">Check your email for the code.</span>
        <input id="auth-code" class="analyse-input analyse-textarea" inputmode="numeric" autocomplete="one-time-code" />
      </div>
      <p id="auth-error" class="analyse-desc auth-error" role="alert"></p>
      <div class="analyse-actions">
        <button id="auth-cancel" type="button" class="btn-ghost">Cancel</button>
        <button id="auth-submit" type="submit" class="btn-primary">Send code</button>
      </div>
    </form>
  </div>`,
);

const modal = document.getElementById("auth-modal")!;
const form = document.getElementById("auth-form") as HTMLFormElement;
const email = document.getElementById("auth-email") as HTMLInputElement;
const code = document.getElementById("auth-code") as HTMLInputElement;
const codeField = document.getElementById("auth-code-field")!;
const error = document.getElementById("auth-error")!;
const submit = document.getElementById("auth-submit") as HTMLButtonElement;

let codeSent = false;

const FRIENDLY: Record<string, string> = {
  signup_disabled: "This email isn't registered. Ask an admin to add you.",
  over_email_send_rate_limit: "Too many attempts. Please wait a few minutes and try again.",
  over_request_rate_limit: "Too many attempts. Please wait a few minutes and try again.",
  otp_expired: "That code is invalid or has expired. Request a new one.",
  validation_failed: "Please enter a valid email address.",
  email_address_invalid: "Please enter a valid email address.",
};

function friendly(err: AuthError): string {
  if (!navigator.onLine || err.name === "AuthRetryableFetchError") return "Network problem. Check your connection and try again.";
  return FRIENDLY[err.code ?? ""] ?? (err.status && err.status >= 500 ? "We couldn't send the email. Please try again later." : "Something went wrong. Please try again.");
}

function reset(): void {
  codeSent = false;
  form.reset();
  codeField.hidden = true;
  email.disabled = false;
  error.textContent = "";
  submit.textContent = "Send code";
}

function close(): void {
  modal.hidden = true;
}

let userEmail: string | undefined;

function renderBtn(): void {
  const icon = document.getElementById("auth-icon")!;
  icon.classList.add("auth-avatar");
  icon.innerHTML = userEmail ? userEmail[0].toUpperCase() : `<i data-lucide="user-round" aria-hidden="true"></i>`;
  document.getElementById("auth-label")!.textContent = userEmail ?? "Log in";
  btn.title = btn.ariaLabel = userEmail ? `Signed in as ${userEmail}` : "Log in";
  btn.ariaHasPopup = "menu";
  refreshIcons();
}

// ponytail: hard-coded plan, replace with the API response when the billing endpoint exists.
const PLAN = "Free";

// Account menu: user, settings, theme, log in / log out. Markup lives in history.html.
const menu = document.getElementById("account-menu")!;
document.body.append(menu); // escape the panel so position:fixed is viewport-relative

function showMenu(): void {
  document.getElementById("menu-email")!.textContent = userEmail ?? "";
  document.getElementById("menu-plan")!.textContent = `${PLAN} plan`;
  for (const id of ["menu-user", "menu-sep", "logout-item"]) document.getElementById(id)!.hidden = !userEmail;
  document.getElementById("login-item")!.hidden = !!userEmail;
  menu.hidden = false;
  const r = btn.getBoundingClientRect();
  menu.style.left = `${r.left}px`;
  menu.style.bottom = `${innerHeight - r.top + 6}px`;
}

// Logout confirmation popup.
document.body.insertAdjacentHTML(
  "beforeend",
  `<div id="logout-modal" class="analyse-overlay" role="alertdialog" aria-modal="true" aria-labelledby="logout-title" hidden>
    <div class="analyse-dialog">
      <div class="analyse-head">
        <h2 id="logout-title" class="analyse-title">Log out</h2>
        <button id="logout-close" type="button" class="analyse-close" aria-label="Close">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </div>
      <div class="analyse-field">
        <p class="analyse-label">Log out of Roofle?</p>
        <span class="analyse-desc">You'll need a new code to sign in again.</span>
      </div>
      <div class="analyse-actions">
        <button id="logout-cancel" type="button" class="btn-ghost">Cancel</button>
        <button id="logout-confirm" type="button" class="btn-primary">Log out</button>
      </div>
    </div>
  </div>`,
);
const logoutModal = document.getElementById("logout-modal")!;

menu.addEventListener("click", (e) => {
  const item = (e.target as HTMLElement).closest<HTMLElement>(".auth-menu-item");
  if (!item || item.id === "theme-toggle") return; // theme stays open so the change is visible
  menu.hidden = true;
  if (item.id === "login-item") openLogin();
  if (item.id === "logout-item") {
    logoutModal.hidden = false;
    document.getElementById("logout-cancel")!.focus();
  }
});
for (const id of ["logout-cancel", "logout-close"]) document.getElementById(id)!.addEventListener("click", () => (logoutModal.hidden = true));
logoutModal.addEventListener("click", (e) => e.target === logoutModal && (logoutModal.hidden = true));
document.getElementById("logout-confirm")!.addEventListener("click", async () => {
  logoutModal.hidden = true;
  await supabase.auth.signOut();
  showToast("Logged out");
});
document.addEventListener("click", (e) => {
  if (!menu.contains(e.target as Node) && !btn.contains(e.target as Node)) menu.hidden = true;
});

function openLogin(): void {
  reset();
  modal.hidden = false;
  email.focus();
}

btn.addEventListener("click", () => (menu.hidden ? showMenu() : (menu.hidden = true)));

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  submit.disabled = true;
  error.textContent = "";
  const { error: err } = codeSent
    ? await supabase.auth.verifyOtp({ email: email.value, token: code.value.trim(), type: "email" })
    : await supabase.auth.signInWithOtp({ email: email.value });
  submit.disabled = false;
  if (err) {
    console.error(err);
    error.textContent = friendly(err);
  } else if (codeSent) {
    close(); // onAuthStateChange updates the button
    showToast("Logged in");
  } else {
    codeSent = true;
    email.disabled = true;
    codeField.hidden = false;
    code.focus();
    submit.textContent = "Log in";
  }
});

for (const id of ["auth-cancel", "auth-close"]) document.getElementById(id)!.addEventListener("click", close);
modal.addEventListener("click", (e) => e.target === modal && close());
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  close();
  menu.hidden = true;
  logoutModal.hidden = true;
});

supabase.auth.onAuthStateChange((_event, session) => {
  userEmail = session?.user.email;
  renderBtn();
});
