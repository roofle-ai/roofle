use std::collections::HashSet;

/// A conferencing app whose calls can be detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallApp {
    Zoom,
    Teams,
}

impl CallApp {
    pub fn name(self) -> &'static str {
        match self {
            CallApp::Zoom => "Zoom",
            CallApp::Teams => "Microsoft Teams",
        }
    }
}

/// Zoom starts this helper only for the length of a meeting.
const ZOOM_MEETING_PROCESSES: &[&str] = &["cpthost", "cpthost.exe"];

/// Teams has no meeting-only process, so it counts as "in a call" only while
/// one of these runs *and* the microphone is capturing.
const TEAMS_PROCESSES: &[&str] = &["msteams", "ms-teams.exe", "teams", "teams.exe"];

/// What the machine looks like right now.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Lowercased names of every running process.
    pub processes: HashSet<String>,
    /// Whether any app is capturing from the default microphone.
    pub mic_active: bool,
}

impl Snapshot {
    fn runs_any(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.processes.contains(*n))
    }
}

/// The call in progress, if any. Zoom wins when both apps are open, since its
/// signal is exact while Teams relies on the shared microphone.
pub fn detect(snapshot: &Snapshot) -> Option<CallApp> {
    if snapshot.runs_any(ZOOM_MEETING_PROCESSES) {
        Some(CallApp::Zoom)
    } else if snapshot.mic_active && snapshot.runs_any(TEAMS_PROCESSES) {
        Some(CallApp::Teams)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(processes: &[&str], mic_active: bool) -> Snapshot {
        Snapshot { processes: processes.iter().map(|p| p.to_string()).collect(), mic_active }
    }

    #[test]
    fn zoom_meeting_helper_means_zoom_call() {
        assert_eq!(detect(&snapshot(&["zoom.us", "cpthost"], false)), Some(CallApp::Zoom));
    }

    #[test]
    fn zoom_open_without_meeting_is_not_a_call() {
        assert_eq!(detect(&snapshot(&["zoom.us"], true)), None);
    }

    #[test]
    fn teams_needs_the_microphone_too() {
        assert_eq!(detect(&snapshot(&["msteams"], false)), None);
        assert_eq!(detect(&snapshot(&["msteams"], true)), Some(CallApp::Teams));
    }

    #[test]
    fn microphone_alone_is_not_a_call() {
        assert_eq!(detect(&snapshot(&["safari"], true)), None);
    }

    #[test]
    fn zoom_wins_over_teams() {
        assert_eq!(detect(&snapshot(&["cpthost", "msteams"], true)), Some(CallApp::Zoom));
    }
}
