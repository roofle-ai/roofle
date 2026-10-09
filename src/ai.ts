// Roofle frontend — typed access to the reusable Rust AI client.
//
// Mirrors roofle_app::ai. The config (base URL / model) is managed from App
// Settings and persisted by Rust; the API key lives in the app_config table
// and Rust falls back to it when a call passes no key.
// Future features call `chat` against the same client.

import { invoke } from "@tauri-apps/api/core";

/** Legacy localStorage key; only read to migrate an old key into the database. */
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

/** Load the AI config (base URL, model, whether a key is saved). */
export async function loadAiConfig(): Promise<AiConfigView> {
  const legacy = loadStoredApiKey();
  if (legacy) {
    // One-time move of a key saved by an older build from localStorage to the database.
    const old = await invoke<AiConfigView>("load_ai_config");
    await saveAiConfig(old.base_url, old.model, legacy);
    try {
      localStorage.removeItem(API_KEY_STORE);
    } catch {
      /* ignore storage failures */
    }
  }
  return invoke<AiConfigView>("load_ai_config");
}

/** Persist the AI config in Rust; a newly typed API key goes to the app_config table. */
export async function saveAiConfig(
  baseUrl: string,
  model: string,
  apiKey: string,
): Promise<void> {
  // The key field is never pre-filled, so blank means "unchanged", not "clear".
  await invoke<void>("save_ai_config", { baseUrl, model, apiKey: apiKey.trim() || null });
}

/** Verify the configured client with a trivial prompt; returns the reply. */
export function testAiConnection(apiKey: string): Promise<string> {
  return invoke<string>("test_ai_connection", { apiKey });
}
