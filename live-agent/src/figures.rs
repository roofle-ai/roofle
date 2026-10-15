//! Key dates, numbers and names pulled from a conversation on demand.
//!
//! Works on a live or finished conversation: the caller supplies the
//! speaker-tagged utterances and gets structured figures back for the UI.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::notes::CompletionClient;
use crate::recap::Utterance;

/// Loaded from `prompts/figures/figures.txt` at compile time.
pub const FIGURES_SYSTEM: &str = include_str!("../prompts/figures/figures.txt");

/// One figure: what it refers to and its value as spoken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Figure {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KeyFigures {
    #[serde(default)]
    pub dates: Vec<Figure>,
    #[serde(default)]
    pub numbers: Vec<Figure>,
    #[serde(default)]
    pub names: Vec<Figure>,
}

impl KeyFigures {
    pub fn is_empty(&self) -> bool {
        self.dates.is_empty() && self.numbers.is_empty() && self.names.is_empty()
    }

    /// Drop items the model left without a value.
    fn cleaned(mut self) -> Self {
        for group in [&mut self.dates, &mut self.numbers, &mut self.names] {
            group.retain(|f| !f.value.trim().is_empty());
        }
        self
    }
}

fn schema() -> serde_json::Value {
    let group = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": { "label": { "type": "string" }, "value": { "type": "string" } },
            "required": ["label", "value"],
            "additionalProperties": false
        }
    });
    json!({
        "type": "object",
        "properties": { "dates": group, "numbers": group, "names": group },
        "required": ["dates", "numbers", "names"],
        "additionalProperties": false
    })
}

/// Extract the key figures from `utterances`. `Ok(None)` when nothing has been said yet.
pub fn extract_figures(
    client: &dyn CompletionClient,
    utterances: &[Utterance],
) -> Result<Option<KeyFigures>, String> {
    let transcript = utterances
        .iter()
        .filter(|u| !u.text.trim().is_empty())
        .map(|u| format!("{}: {}", u.speaker, u.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.is_empty() {
        return Ok(None);
    }
    let reply = client.complete_json(FIGURES_SYSTEM, &format!("Transcript:\n{transcript}"), &schema())?;
    parse(&reply).map(Some)
}

/// Parse the reply, tolerating prose or a code fence around the JSON object.
fn parse(raw: &str) -> Result<KeyFigures, String> {
    let raw = raw.trim();
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').map_or(raw.len(), |e| e + 1);
    serde_json::from_str::<KeyFigures>(&raw[start..end])
        .map(KeyFigures::cleaned)
        .map_err(|e| format!("bad key-figures JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> Utterance {
        Utterance { speaker: "Others", text: text.to_string(), end_ms: 1000 }
    }

    struct Reply(&'static str);

    impl CompletionClient for Reply {
        fn complete(&self, _: &str, _: &str) -> Result<String, String> {
            Ok(self.0.to_string())
        }
    }

    #[test]
    fn parses_groups_and_drops_valueless_items() {
        let reply = Reply(r#"```json
{"dates":[{"label":"Launch","value":"March 14"},{"label":"Blank","value":" "}],"numbers":[{"label":"Seats","value":"about 30"}],"names":[]}
```"#);
        let figures = extract_figures(&reply, &[said("hi")]).unwrap().unwrap();
        assert_eq!(figures.dates, vec![Figure { label: "Launch".into(), value: "March 14".into() }]);
        assert_eq!(figures.numbers.len(), 1);
        assert!(figures.names.is_empty());
    }

    #[test]
    fn missing_arrays_default_to_empty() {
        let figures = extract_figures(&Reply(r#"{"dates":[]}"#), &[said("hi")]).unwrap().unwrap();
        assert!(figures.is_empty());
    }

    #[test]
    fn nothing_said_means_no_call_and_no_result() {
        assert_eq!(extract_figures(&Reply("unused"), &[said("  ")]).unwrap(), None);
        assert!(extract_figures(&Reply("not json"), &[said("hi")]).is_err());
    }
}
