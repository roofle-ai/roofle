use std::sync::mpsc::Sender;

pub type Reply = Sender<Result<(), String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Mic,
    System,
    Both,
}

impl SourceKind {
    /// System capture (and so `System` and `Both`) exists on macOS only.
    pub fn is_implemented(self) -> bool {
        match self {
            SourceKind::Mic => true,
            SourceKind::System | SourceKind::Both => cfg!(target_os = "macos"),
        }
    }

    pub fn as_db_str(self) -> &'static str {
        match self {
            SourceKind::Mic => "mic",
            SourceKind::System => "system",
            SourceKind::Both => "both",
        }
    }
}

/// Messages sent to the engine thread.
pub enum Control {
    /// `session_id` continues a stored session instead of opening a new one.
    Start {
        source: SourceKind,
        session_id: Option<i64>,
        reply: Reply,
    },
    Stop { reply: Reply },
    /// Capture keeps running but audio stops feeding the ASR session.
    Pause { reply: Reply },
    Resume { reply: Reply },
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_parses_lowercase() {
        let mic: SourceKind = serde_json::from_str("\"mic\"").unwrap();
        let system: SourceKind = serde_json::from_str("\"system\"").unwrap();
        let both: SourceKind = serde_json::from_str("\"both\"").unwrap();
        assert_eq!(mic, SourceKind::Mic);
        assert_eq!(system, SourceKind::System);
        assert_eq!(both, SourceKind::Both);
    }

    #[test]
    fn source_implementation_matrix() {
        assert!(SourceKind::Mic.is_implemented());
        assert_eq!(SourceKind::System.is_implemented(), cfg!(target_os = "macos"));
        assert_eq!(SourceKind::Both.is_implemented(), cfg!(target_os = "macos"));
    }
}
