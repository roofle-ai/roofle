// Roofle frontend — typed access to the reusable Rust AI client.
//
// Mirrors roofle_app::ai. The config (base URL / model / API key) is managed
// from App Settings; future features call `chat` against the same client.

import { invoke } from "@tauri-apps/api/core";

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

/** Load the AI config (base URL, model, whether a key is set). */
export function loadAiConfig(): Promise<AiConfigView> {
  return invoke<AiConfigView>("load_ai_config");
}

/** Persist the AI config; pass an empty key to clear the stored key. */
export function saveAiConfig(
  baseUrl: string,
  model: string,
  apiKey: string,
): Promise<void> {
  return invoke<void>("save_ai_config", { baseUrl, model, apiKey });
}

/** Verify the configured client with a trivial prompt; returns the reply. */
export function testAiConnection(): Promise<string> {
  return invoke<string>("test_ai_connection");
}
