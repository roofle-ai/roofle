//! Reusable user persona (not tied to any one agent): turns the saved persona into
//! one deterministic first-person paragraph ("I am a ..., I primarily take meetings on ...").
//! No LLM, no I/O: same inputs always give the same text.

use serde::{Deserialize, Serialize};

/// Display strings, already resolved from the stored ids (role label, meeting labels).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersonaProfile {
    pub role: String,
    pub meetings: Vec<String>,
    /// What this role cares about in a conversation (from the role's data).
    #[serde(default)]
    pub focus: Vec<String>,
    /// User-written summary; replaces the generated role/meetings/focus text when non-empty.
    #[serde(default)]
    pub summary: String,
}

impl PersonaProfile {
    /// Empty when the user skipped persona setup and no goal is known.
    pub fn describe(&self, goal: &str) -> String {
        let role = self.role.trim();
        let meetings: Vec<String> = self.meetings.iter().map(|m| lower_first(m.trim())).filter(|m| !m.is_empty()).collect();
        let focus: Vec<String> = self.focus.iter().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
        let goal = goal.trim().trim_end_matches('.');

        let mut out = Vec::new();
        if !self.summary.trim().is_empty() {
            out.push(self.summary.trim().to_string());
            if !goal.is_empty() {
                out.push(format!("My goal in this conversation is: {goal}."));
            }
            return out.join(" ");
        }
        match (role.is_empty(), meetings.is_empty()) {
            (false, false) => out.push(format!("I am {} {role} and I primarily take meetings on {}.", article(role), join(&meetings))),
            (false, true) => out.push(format!("I am {} {role}.", article(role))),
            (true, false) => out.push(format!("I primarily take meetings on {}.", join(&meetings))),
            (true, true) => {}
        }
        if !focus.is_empty() {
            out.push(format!("In these conversations I care most about {}.", join(&focus)));
        }
        if !goal.is_empty() {
            out.push(format!("My goal in this conversation is: {goal}."));
        }
        out.join(" ")
    }
}

fn article(word: &str) -> &'static str {
    if word.chars().next().is_some_and(|c| "aeiouAEIOU".contains(c)) { "an" } else { "a" }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_lowercase().chain(c).collect())
}

/// "a", "a and b", "a, b and c".
fn join(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(role: &str, meetings: &[&str]) -> PersonaProfile {
        PersonaProfile { role: role.into(), meetings: meetings.iter().map(|s| s.to_string()).collect(), focus: vec![], summary: String::new() }
    }

    #[test]
    fn full_paragraph() {
        let t = p("Engineer / Tech Lead", &["Design & code reviews", "Architecture decisions", "Sprint planning"])
            .describe("Decide whether to move off Glue.");
        assert_eq!(
            t,
            "I am an Engineer / Tech Lead and I primarily take meetings on design & code reviews, architecture decisions and sprint planning. My goal in this conversation is: Decide whether to move off Glue."
        );
    }

    #[test]
    fn focus_sits_between_meetings_and_goal() {
        let mut persona = p("Recruiter / HR", &["Candidate interviews"]);
        persona.focus = vec!["real ownership".into(), "fit for the role".into()];
        assert_eq!(
            persona.describe("Judge fit."),
            "I am a Recruiter / HR and I primarily take meetings on candidate interviews. In these conversations I care most about real ownership and fit for the role. My goal in this conversation is: Judge fit."
        );
    }

    #[test]
    fn custom_summary_replaces_generated_text() {
        let mut persona = p("Recruiter / HR", &["Candidate interviews"]);
        persona.summary = "I hire designers.".into();
        assert_eq!(persona.describe("Judge fit."), "I hire designers. My goal in this conversation is: Judge fit.");
    }

    #[test]
    fn missing_parts_are_skipped() {
        assert_eq!(p("", &[]).describe(""), "");
        assert_eq!(p("Recruiter / HR", &[]).describe(""), "I am a Recruiter / HR.");
        assert_eq!(p("", &["Sales calls"]).describe("close"), "I primarily take meetings on sales calls. My goal in this conversation is: close.");
    }
}
