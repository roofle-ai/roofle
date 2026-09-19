// Roofle frontend — typed access to the reusable Rust AI client.
//
// Mirrors roofle_app::ai. The config (base URL / model) is managed from App
// Settings and persisted by Rust; the API key is owned by this renderer in
// localStorage (kept out of the OS keychain) and handed back to Rust per call.
// Future features call `chat` against the same client.

import { invoke } from "@tauri-apps/api/core";

/** localStorage key holding the API key (owned by the renderer, not keychain). */
export const API_KEY_STORE = "roofle-ai-api-key";

export interface AiConfigView {
  kind: "openai_compat";
  base_url: string;
  model: string;
  has_api_key: boolean;
}

export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

/** Read the stored API key from localStorage ("" when none is saved). */
export function loadStoredApiKey(): string {
  try {
    return localStorage.getItem(API_KEY_STORE) ?? "";
  } catch {
    return "";
  }
}

/** Persist the API key to localStorage ("" clears it). Never touches keychain. */
export function storeApiKey(key: string): void {
  try {
    localStorage.setItem(API_KEY_STORE, key.trim());
  } catch {
    /* ignore storage failures */
  }
}

/** Load the AI config (base URL, model). Key presence comes from localStorage. */
export async function loadAiConfig(): Promise<AiConfigView> {
  const cfg = await invoke<AiConfigView>("load_ai_config");
  cfg.has_api_key = loadStoredApiKey().length > 0;
  return cfg;
}

/** Persist the non-secret AI config in Rust; store the API key in localStorage. */
export async function saveAiConfig(
  baseUrl: string,
  model: string,
  apiKey: string,
): Promise<void> {
  await invoke<void>("save_ai_config", { baseUrl, model });
  storeApiKey(apiKey);
}

/** Verify the configured client with a trivial prompt; returns the reply. */
export function testAiConnection(apiKey: string): Promise<string> {
  return invoke<string>("test_ai_connection", { apiKey });
}
