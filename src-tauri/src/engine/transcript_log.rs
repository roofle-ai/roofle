use chrono::Utc;
use live_agent::notes::logging_enabled;
use roofle_core::storage::AudioSource;

use crate::dev_log::RunLog;

static TRANSCRIPT_LOG: RunLog = RunLog::new("roofle-transcript");

/// Dev aid: mirror each finalized utterance to a local file when
/// `ROOFLE_LOG_AI` is on.
pub fn log_utterance(source: AudioSource, text: &str) {
    if !logging_enabled() {
        return;
    }
    let tag = match source {
        AudioSource::Mic => "Mic",
        AudioSource::System => "System",
    };
    TRANSCRIPT_LOG.append(&format!("[{}] [{tag}] {text}\n", Utc::now().to_rfc3339()));
}
