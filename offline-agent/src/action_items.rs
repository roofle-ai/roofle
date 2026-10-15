//! Action items parsed from the notes' `## Action Items` section.
//!
//! The notes writer records them as `Owner: task — by date` (owner and date
//! only when stated), so extraction is a deterministic parse: instant, free,
//! and it never invents an owner or a date.

use serde::Serialize;

use crate::sections::section_lines;

/// One commitment from the conversation. `owner` and `due` are `None` when the
/// notes did not state them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionItem {
    pub owner: Option<String>,
    pub task: String,
    pub due: Option<String>,
}

/// Longest owner prefix, in words, before a colon is treated as part of the task.
// ponytail: word-count heuristic; a short task label before ':' reads as an owner. Ask the LLM if that misfires.
const MAX_OWNER_WORDS: usize = 3;

/// Parse every action item in `notes`, in document order.
pub fn parse_action_items(notes: &str) -> Vec<ActionItem> {
    section_lines(notes, "Action Items")
        .into_iter()
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<ActionItem> {
    let line = strip_marker(line);
    if line.is_empty() || line.eq_ignore_ascii_case("none") || line.to_ascii_lowercase().starts_with("no action") {
        return None;
    }
    let (body, due) = split_due(line);
    let (owner, task) = split_owner(body);
    (!task.is_empty()).then(|| ActionItem {
        owner: owner.map(str::to_string),
        task: task.to_string(),
        due: due.map(str::to_string),
    })
}

/// Drop a leading bullet, number or checkbox: `-`, `*`, `•`, `1.`, `[ ]`.
fn strip_marker(line: &str) -> &str {
    let line = line.trim_start_matches(['-', '*', '•']).trim_start();
    let line = line
        .split_once(". ")
        .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        .map_or(line, |(_, rest)| rest);
    line.trim_start_matches("[ ]").trim_start_matches("[x]").trim()
}

/// Split `task — by date` on the last spaced dash.
fn split_due(line: &str) -> (&str, Option<&str>) {
    let Some((body, due)) = [" — ", " – "].iter().find_map(|d| line.rsplit_once(d)) else {
        return (line, None);
    };
    let due = due.trim();
    let due = due.strip_prefix("by ").or_else(|| due.strip_prefix("By ")).unwrap_or(due).trim();
    (body.trim(), (!due.is_empty()).then_some(due))
}

/// Split `Owner: task` when the prefix is short enough to be a name.
fn split_owner(body: &str) -> (Option<&str>, &str) {
    match body.split_once(':') {
        Some((owner, task)) if (1..=MAX_OWNER_WORDS).contains(&owner.split_whitespace().count()) => {
            (Some(owner.trim()), task.trim())
        }
        _ => (None, body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(owner: Option<&str>, task: &str, due: Option<&str>) -> ActionItem {
        ActionItem { owner: owner.map(Into::into), task: task.into(), due: due.map(Into::into) }
    }

    #[test]
    fn parses_owner_task_and_due() {
        let notes = "## Action Items\n- Sam: draft the spec — by Friday\n- Send pricing deck\n- Priya: book room — Oct 3\n";
        assert_eq!(
            parse_action_items(notes),
            vec![
                item(Some("Sam"), "draft the spec", Some("Friday")),
                item(None, "Send pricing deck", None),
                item(Some("Priya"), "book room", Some("Oct 3")),
            ]
        );
    }

    #[test]
    fn long_prefix_is_not_an_owner() {
        let items = parse_action_items("## Action Items\n- Follow up on the pricing question: tiers\n");
        assert_eq!(items, vec![item(None, "Follow up on the pricing question: tiers", None)]);
    }

    #[test]
    fn handles_numbering_checkboxes_and_none() {
        let notes = "## Action Items\n1. Ana: ship it\n- [ ] Bo: test it\n";
        assert_eq!(parse_action_items(notes).len(), 2);
        assert!(parse_action_items("## Action Items\nNone\n").is_empty());
        assert!(parse_action_items("## Summary\nx").is_empty());
    }
}
