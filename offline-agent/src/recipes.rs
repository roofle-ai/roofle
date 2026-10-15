//! Post-call "recipes": quick drafts generated from the finished notes.
//!
//! Each recipe is one prompt file plus one LLM call. Adding a recipe means a
//! new variant, its prompt, and a line in [`Recipe::system`]; callers and
//! [`run`] stay untouched. Input is the notes (a few hundred words), never the
//! transcript, so a recipe is fast and cheap.

use serde::{Deserialize, Serialize};

use live_agent::notes::CompletionClient;

const RECAP_EMAIL_SYSTEM: &str = include_str!("../prompts/recipes/recap-email.txt");
const SLACK_POST_SYSTEM: &str = include_str!("../prompts/recipes/slack-post.txt");

/// Which draft to produce. Serialized as kebab-case for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Recipe {
    RecapEmail,
    SlackPost,
}

impl Recipe {
    /// Stable identifier, also the storage key for generated drafts.
    pub fn key(self) -> &'static str {
        match self {
            Recipe::RecapEmail => "recap-email",
            Recipe::SlackPost => "slack-post",
        }
    }

    fn system(self) -> &'static str {
        match self {
            Recipe::RecapEmail => RECAP_EMAIL_SYSTEM,
            Recipe::SlackPost => SLACK_POST_SYSTEM,
        }
    }
}

/// A ready-to-paste draft. `subject` is set only for email.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Draft {
    pub subject: Option<String>,
    pub body: String,
}

/// Draft `recipe` from the session `notes`; `goal` is the call's purpose (may be empty).
pub fn run(
    client: &dyn CompletionClient,
    recipe: Recipe,
    notes: &str,
    goal: &str,
) -> Result<Draft, String> {
    if notes.trim().is_empty() {
        return Err("there are no notes to draft from yet".to_string());
    }
    let user = format!("Goal: {}\n\nNotes:\n{notes}", if goal.trim().is_empty() { "(not stated)" } else { goal.trim() });
    let reply = client.complete(recipe.system(), &user)?;
    Ok(parse_draft(recipe, &reply))
}

/// Email replies lead with `Subject: …`; split it from the body.
fn parse_draft(recipe: Recipe, reply: &str) -> Draft {
    let reply = reply.trim();
    if recipe == Recipe::RecapEmail {
        if let Some((first, rest)) = reply.split_once('\n') {
            if let Some(subject) = first.trim().strip_prefix("Subject:") {
                return Draft { subject: Some(subject.trim().to_string()), body: rest.trim().to_string() };
            }
        }
    }
    Draft { subject: None, body: reply.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(&'static str);

    impl CompletionClient for Echo {
        fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            Ok(self.0.to_string())
        }
    }

    #[test]
    fn email_splits_subject_from_body() {
        let client = Echo("Subject: Q3 plan recap\n\nHi all,\nThanks.");
        let draft = run(&client, Recipe::RecapEmail, "## Summary\nx", "").unwrap();
        assert_eq!(draft.subject.as_deref(), Some("Q3 plan recap"));
        assert_eq!(draft.body, "Hi all,\nThanks.");
    }

    #[test]
    fn key_matches_serde_name() {
        for r in [Recipe::RecapEmail, Recipe::SlackPost] {
            let parsed: Recipe = serde_json::from_str(&format!("\"{}\"", r.key())).unwrap();
            assert_eq!(parsed, r);
        }
    }

    #[test]
    fn slack_keeps_whole_reply() {
        let client = Echo("Subject: not a subject\nbody");
        let draft = run(&client, Recipe::SlackPost, "## Summary\nx", "").unwrap();
        assert_eq!(draft, Draft { subject: None, body: "Subject: not a subject\nbody".into() });
    }

    #[test]
    fn empty_notes_are_rejected_without_calling_the_model() {
        assert!(run(&Echo("unused"), Recipe::SlackPost, "  ", "").is_err());
    }
}
