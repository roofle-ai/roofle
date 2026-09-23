//! Offline analysis logic — pure, HTTP-free, driven by a [`CompletionClient`].
//!
//! ```text
//! transcript + perspective + context
//!   -> LLM (prompts/sales-seller.txt | sales-buyer.txt)
//!   -> AnalysisResult { call_summary, caveats, dimensions, overall_score, ... }
//! ```

use serde::{Deserialize, Serialize};

use live_agent::notes::CompletionClient;

/// Evaluates a conversation from the perspective of the person selling.
/// Loaded from `prompts/sales-seller.txt` at compile time.
pub const SALES_SELLER_SYSTEM: &str = include_str!("../prompts/sales-seller.txt");

/// Evaluates a conversation from the perspective of the person buying.
/// Loaded from `prompts/sales-buyer.txt` at compile time.
pub const SALES_BUYER_SYSTEM: &str = include_str!("../prompts/sales-buyer.txt");

/// One scored call dimension (0-100). `score` is `null` when the transcript
/// had too little evidence to score that dimension honestly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// A full sales-call self-assessment, ready for the dashboard. Mirrors the
/// output schema in `prompts/sales-seller.txt` / `sales-buyer.txt`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    #[serde(default, rename = "call_summary")]
    pub call_summary: String,
    #[serde(default)]
    pub caveats: Vec<String>,
    #[serde(default)]
    pub dimensions: std::collections::HashMap<String, Dimension>,
    #[serde(default, rename = "lowest_scoring_dimension")]
    pub lowest_scoring_dimension: Option<String>,
    #[serde(default, rename = "overall_score")]
    pub overall_score: f64,
}

/// Pick the system prompt for a context. Selling and buying use dedicated
/// sales prompts; anything unrecognised falls back to the seller lens.
fn system_for(context: &str) -> &'static str {
    if context.to_ascii_lowercase().contains("buying") {
        SALES_BUYER_SYSTEM
    } else {
        SALES_SELLER_SYSTEM
    }
}

/// Evaluate `transcript` from the given `perspective` with `context` background.
pub fn analyse(
    client: &dyn CompletionClient,
    transcript: &str,
    perspective: &str,
    context: &str,
) -> Result<AnalysisResult, String> {
    let system = system_for(context);
    let user = build_user(system, transcript, perspective, context);
    let reply = client.complete(system, &user)?;
    parse_json(&reply)
}

/// Build the user message. The seller prompt is grounded in the whole
/// conversation, so the transcript is repeated into the context section too
/// (the prompt file itself is left untouched).
fn build_user(system: &str, transcript: &str, perspective: &str, context: &str) -> String {
    let base = format!(
        "Perspective: {perspective}\n\nContext: {context}\n\nTranscript:\n{transcript}"
    );
    if system == SALES_SELLER_SYSTEM {
        format!("{base}\n\nContext (full conversation):\n{transcript}")
    } else {
        base
    }
}

/// Parse the agent's JSON, tolerating prose or a markdown code fence around it.
/// Empty replies yield an empty (default) result rather than a hard error.
fn parse_json<T: for<'de> Deserialize<'de> + Default>(raw: &str) -> Result<T, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(T::default());
    }
    let start = trimmed.find('{').unwrap_or(0);
    let end = trimmed.rfind('}').map(|e| e + 1).unwrap_or(trimmed.len());
    serde_json::from_str(&trimmed[start..end]).map_err(|e| format!("bad analysis JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub client returning a full, well-formed analysis JSON.
    struct Stub;

    impl CompletionClient for Stub {
        fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            Ok(
                r#"{"call_summary":"Rough call, little structure.","caveats":["Transcript fragmented."],"dimensions":{"talk_listen_balance":{"score":50,"reasoning":"Unbalanced talk.","evidence":["Rep monologued."]},"discovery_depth":{"score":null,"reasoning":"No pain points surfaced.","evidence":[]}},"lowest_scoring_dimension":"discovery_depth","overall_score":50}"#
                    .to_string(),
            )
        }
    }

    #[test]
    fn parses_analysis_json() {
        let result = analyse(&Stub, "hi", "sales", "Selling a product").unwrap();
        assert_eq!(result.call_summary, "Rough call, little structure.");
        assert_eq!(result.caveats.len(), 1);
        assert_eq!(result.dimensions.len(), 2);
        assert_eq!(result.dimensions["talk_listen_balance"].score, Some(50.0));
        // Null dimension scores round-trip as None.
        assert_eq!(result.dimensions["discovery_depth"].score, None);
        assert_eq!(result.lowest_scoring_dimension.as_deref(), Some("discovery_depth"));
        assert_eq!(result.overall_score, 50.0);
    }

    /// A stub that wraps its answer in a markdown fence.
    struct Fenced;

    impl CompletionClient for Fenced {
        fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            Ok("Here you go:\n```json\n{\"call_summary\":\"ok\",\"overall_score\":80}\n```\nBye.".to_string())
        }
    }

    #[test]
    fn tolerates_code_fence_and_missing_fields() {
        let result = analyse(&Fenced, "hi", "sales", "Selling a product").unwrap();
        assert_eq!(result.call_summary, "ok");
        assert_eq!(result.overall_score, 80.0);
        // Missing fields fall back to defaults instead of failing.
        assert!(result.caveats.is_empty());
        assert!(result.dimensions.is_empty());
        assert_eq!(result.lowest_scoring_dimension, None);
    }

    /// Persistence writes `AnalysisResult` via `serde_json::to_string` and reads
    /// it back with `from_str`; this must be a lossless round trip.
    #[test]
    fn analysis_result_round_trips_through_json() {
        let result = analyse(&Stub, "hi", "sales", "Selling a product").unwrap();

        let json = serde_json::to_string(&result).unwrap();
        let back: AnalysisResult = serde_json::from_str(&json).unwrap();

        assert_eq!(back.call_summary, result.call_summary);
        assert_eq!(back.overall_score, result.overall_score);
        assert_eq!(back.lowest_scoring_dimension, result.lowest_scoring_dimension);
        assert_eq!(back.dimensions.len(), result.dimensions.len());
        // Null and numeric dimension scores survive the round trip.
        assert_eq!(back.dimensions["talk_listen_balance"].score, Some(50.0));
        assert_eq!(back.dimensions["discovery_depth"].score, None);
        assert_eq!(back.dimensions["discovery_depth"].reasoning, "No pain points surfaced.");
    }
}
