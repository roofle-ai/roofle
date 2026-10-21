// A small confirmation modal (reuses the app's modal styles), built on first use.
let modal: HTMLElement;
let titleEl: HTMLElement;
let messageEl: HTMLElement;
let confirmBtn: HTMLButtonElement;
let settle: ((ok: boolean) => void) | null = null;

function close(ok: boolean): void {
  modal.hidden = true;
  settle?.(ok);
  settle = null;
}

function build(): void {
  document.body.insertAdjacentHTML(
    "beforeend",
    `<div id="confirm-modal" class="analyse-overlay" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-message" hidden>
      <div class="analyse-dialog">
        <div class="analyse-head"><h2 id="confirm-title" class="analyse-title"></h2></div>
        <p id="confirm-message" class="analyse-desc"></p>
        <div class="analyse-actions">
          <button id="confirm-cancel" type="button" class="btn-ghost">Cancel</button>
          <button id="confirm-ok" type="button" class="btn-primary btn-danger"></button>
        </div>
      </div>
    </div>`,
  );
  modal = document.getElementById("confirm-modal")!;
  titleEl = document.getElementById("confirm-title")!;
  messageEl = document.getElementById("confirm-message")!;
  confirmBtn = document.getElementById("confirm-ok") as HTMLButtonElement;
  document.getElementById("confirm-cancel")!.addEventListener("click", () => close(false));
  confirmBtn.addEventListener("click", () => close(true));
  modal.addEventListener("click", (e) => e.target === modal && close(false));
  window.addEventListener("keydown", (e) => e.key === "Escape" && !modal.hidden && close(false));
}

/** Ask the user to confirm a destructive action; resolves true only on the confirm button. */
export function confirmDialog(title: string, message: string, confirmLabel: string): Promise<boolean> {
  if (!modal) build();
  titleEl.textContent = title;
  messageEl.textContent = message;
  confirmBtn.textContent = confirmLabel;
  modal.hidden = false;
  confirmBtn.focus();
  return new Promise((resolve) => (settle = resolve));
}
