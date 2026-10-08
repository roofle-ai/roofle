//! Pipeline tunables. Defaults follow the design doc; a session may override
//! any field by building its own value.

/// Hard ceiling for `max_open_questions`, whatever the setting says.
pub const MAX_OPEN_CAP: usize = 5;

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub hold_seconds: i64,
    pub max_open_questions: usize,
    pub min_gap_between_questions_s: i64,
    pub stale_after_s: i64,
    pub warmup_chunks: u32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            hold_seconds: 30,
            max_open_questions: 2,
            min_gap_between_questions_s: 60,
            stale_after_s: 600,
            warmup_chunks: 2,
        }
    }
}
