//! On-demand recap of the most recent slice of a live conversation.
//!
//! The caller supplies the speaker-tagged utterances; this module picks the
//! trailing time window, formats it for the LLM and returns the summary. When
//! the conversation is younger than the window, the whole conversation is used
//! and [`Recap::covered_secs`] reports the shorter span actually covered.

use serde::Serialize;

use crate::notes::CompletionClient;

/// Steers the LLM toward a short, faithful catch-up. Loaded from
/// `prompts/recap/recap.txt` at compile time.
pub const RECAP_SYSTEM: &str = include_str!("../prompts/recap/recap.txt");

/// One recognised utterance, with offsets from the session start.
#[derive(Debug, Clone)]
pub struct Utterance {
    /// Who spoke, as shown to the model ("You", "Others").
    pub speaker: &'static str,
    pub text: String,
    pub end_ms: i64,
}

/// The LLM's recap plus how much conversation it is based on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recap {
    pub summary: String,
    /// Seconds of conversation covered: the requested window, or less when the
    /// conversation is shorter than that.
    pub covered_secs: u32,
}

/// Recap the last `window_secs` of `utterances`. `Ok(None)` when nothing has
/// been said yet.
pub fn recap_recent(
    client: &dyn CompletionClient,
    utterances: &[Utterance],
    window_secs: u32,
) -> Result<Option<Recap>, String> {
    let Some((excerpt, covered_secs)) = select_window(utterances, window_secs) else {
        return Ok(None);
    };
    let summary = client.complete(RECAP_SYSTEM, &format!("Transcript:\n{excerpt}"))?;
    Ok(Some(Recap { summary: summary.trim().to_string(), covered_secs }))
}

/// The transcript excerpt for the trailing window and the seconds it covers.
pub(crate) fn select_window(utterances: &[Utterance], window_secs: u32) -> Option<(String, u32)> {
    let spoken: Vec<&Utterance> = utterances.iter().filter(|u| !u.text.trim().is_empty()).collect();
    let latest_ms = spoken.iter().map(|u| u.end_ms).max()?;
    let window_ms = i64::from(window_secs) * 1000;
    let cutoff_ms = latest_ms - window_ms;

    let excerpt = spoken
        .iter()
        .filter(|u| u.end_ms > cutoff_ms)
        .map(|u| format!("{}: {}", u.speaker, u.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    // A conversation shorter than the window is covered from its very start.
    let covered_secs = (latest_ms.min(window_ms) / 1000).max(1) as u32;
    Some((excerpt, covered_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(speaker: &'static str, text: &str, end_ms: i64) -> Utterance {
        Utterance { speaker, text: text.to_string(), end_ms }
    }

    struct Echo;

    impl CompletionClient for Echo {
        fn complete(&self, _system: &str, user: &str) -> Result<String, String> {
            Ok(format!("  {user}  "))
        }
    }

    #[test]
    fn keeps_only_the_trailing_window() {
        let lines = [said("Others", "old", 10_000), said("You", "mid", 35_000), said("Others", "new", 70_000)];
        let (excerpt, covered) = select_window(&lines, 30).unwrap();
        assert_eq!(excerpt, "Others: new");
        assert_eq!(covered, 30);
    }

    #[test]
    fn short_conversation_uses_everything_and_reports_real_span() {
        let lines = [said("Others", "hello", 6_000), said("You", "hi there", 18_000)];
        let (excerpt, covered) = select_window(&lines, 30).unwrap();
        assert_eq!(excerpt, "Others: hello\nYou: hi there");
        assert_eq!(covered, 18);
    }

    #[test]
    fn nothing_said_yields_no_recap() {
        assert_eq!(recap_recent(&Echo, &[], 30).unwrap(), None);
        assert_eq!(recap_recent(&Echo, &[said("You", "   ", 5_000)], 30).unwrap(), None);
    }

    #[test]
    fn summary_is_trimmed() {
        let recap = recap_recent(&Echo, &[said("You", "hi", 4_000)], 30).unwrap().unwrap();
        assert_eq!(recap.summary, "Transcript:\nYou: hi");
        assert_eq!(recap.covered_secs, 4);
    }
}
