//! Read-only access to the `## Heading` sections of the markdown notes
//! produced by the live notes agent (see `live-agent/prompts/writer.txt`).

/// Non-empty, trimmed lines under `## <name>` (case-insensitive), up to the
/// next `## ` heading. Empty when the section is absent.
pub fn section_lines<'a>(notes: &'a str, name: &str) -> Vec<&'a str> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in notes.lines().map(str::trim) {
        if let Some(heading) = line.strip_prefix("## ") {
            in_section = heading.trim().eq_ignore_ascii_case(name);
        } else if in_section && !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// First line of the Summary section, for short previews (e.g. notifications).
pub fn summary_line(notes: &str) -> Option<&str> {
    section_lines(notes, "Summary").into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str = "## Summary\nShip plan agreed.\nMore.\n\n## Decisions\n- Use Postgres\n## Action Items\n- Sam: draft spec — by Friday\n";

    #[test]
    fn reads_named_section_only() {
        assert_eq!(section_lines(NOTES, "decisions"), vec!["- Use Postgres"]);
        assert_eq!(section_lines(NOTES, "Action Items"), vec!["- Sam: draft spec — by Friday"]);
        assert!(section_lines(NOTES, "Timeline").is_empty());
    }

    #[test]
    fn summary_line_is_first_line() {
        assert_eq!(summary_line(NOTES), Some("Ship plan agreed."));
        assert_eq!(summary_line("no headings"), None);
    }
}
