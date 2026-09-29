//! Post-meeting title generation.
//!
//! Runs once when a recording stops: reads the meeting notes and asks the LLM
//! for a short, descriptive conversation title (5-6 words) to store on the
//! session row.

use live_agent::notes::CompletionClient;

/// Steers the LLM toward a concise conversation title.
/// Loaded from `metadata/title-generator.txt` at compile time.
pub const TITLE_SYSTEM: &str = include_str!("../metadata/title-generator.txt");

/// Generate a conversation title from `notes`. Returns the trimmed title
/// (may be empty if the model replied with nothing usable).
pub fn generate_title(client: &dyn CompletionClient, notes: &str) -> Result<String, String> {
    let user = format!("Meeting notes:\n{notes}");
    let reply = client.complete(TITLE_SYSTEM, &user)?;
    Ok(parse_title(&reply))
}

/// Strip quotes, surrounding whitespace and a trailing period from the reply.
fn parse_title(raw: &str) -> String {
    raw.trim()
        .trim_matches(['"', '\''].as_ref())
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub that returns a title wrapped in quotes and a period.
    struct Stub;

    impl CompletionClient for Stub {
        fn complete(&self, _system: &str, _user: &str) -> Result<String, String> {
            Ok("\"Q3 Roadmap Planning.\"".to_string())
        }
    }

    #[test]
    fn strips_quotes_and_punctuation() {
        let title = generate_title(&Stub, "notes").unwrap();
        assert_eq!(title, "Q3 Roadmap Planning");
    }

    #[test]
    fn prompt_embedded_at_compile_time() {
        assert!(TITLE_SYSTEM.contains("5 to 6 words"), "prompt missing constraint");
    }
}
