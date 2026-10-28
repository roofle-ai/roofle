//! One-click assist prompts ("What should I ask next?", "Say it simpler", ...).
//!
//! Each prompt id maps to a system prompt in `prompts/assist/` and the trailing
//! transcript window it needs. The answer is plain text for the UI to show.

use crate::notes::CompletionClient;
use crate::recap::{select_window, Utterance};

/// (system prompt, seconds of transcript) for a prompt id from `src/data/assist-prompts.json`.
fn prompt_for(id: &str) -> Option<(&'static str, u32)> {
    Some(match id {
        "next-question" => (include_str!("../prompts/assist/next-question.txt"), 60),
        "say-simpler" => (include_str!("../prompts/assist/say-simpler.txt"), 60),
        "probe-interests" => (include_str!("../prompts/assist/probe-interests.txt"), 300),
        "confirm-understanding" => (include_str!("../prompts/assist/confirm-understanding.txt"), 300),
        _ => return None,
    })
}

/// Answer assist prompt `id` from `utterances`. `Ok(None)` when nothing has been said yet.
pub fn run_assist(
    client: &dyn CompletionClient,
    id: &str,
    utterances: &[Utterance],
) -> Result<Option<String>, String> {
    let (system, window_secs) = prompt_for(id).ok_or_else(|| format!("unknown assist prompt: {id}"))?;
    let Some((excerpt, _)) = select_window(utterances, window_secs) else { return Ok(None) };
    let answer = client.complete(system, &format!("Transcript:\n{excerpt}"))?;
    Ok(Some(answer.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl CompletionClient for Echo {
        fn complete(&self, system: &str, user: &str) -> Result<String, String> {
            Ok(format!("{}|{user}", system.lines().next().unwrap_or("")))
        }
    }

    fn said(text: &str, end_ms: i64) -> Utterance {
        Utterance { speaker: "Others", text: text.to_string(), end_ms }
    }

    #[test]
    fn every_prompt_in_the_json_has_a_system_prompt() {
        let json = include_str!("../../src/data/assist-prompts.json");
        let ids: Vec<&str> = json.lines().filter_map(|l| l.trim().strip_prefix("\"id\": \"")).filter_map(|l| l.split('"').next()).collect();
        for id in ids.iter().filter(|i| !["respond-well", "understand-position", "stay-on-track"].contains(i)) {
            assert!(prompt_for(id).is_some(), "no prompt for {id}");
        }
    }

    #[test]
    fn unknown_id_errors_and_empty_transcript_yields_none() {
        assert!(run_assist(&Echo, "nope", &[]).is_err());
        assert_eq!(run_assist(&Echo, "say-simpler", &[]).unwrap(), None);
    }

    #[test]
    fn answers_from_the_transcript() {
        let out = run_assist(&Echo, "next-question", &[said("we have a budget", 5_000)]).unwrap().unwrap();
        assert!(out.ends_with("Transcript:\nOthers: we have a budget"));
    }
}
