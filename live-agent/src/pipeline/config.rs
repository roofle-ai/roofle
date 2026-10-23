//! Pipeline tunables. Defaults follow the design doc; a session may override
//! any field by building its own value.

use super::stages::MIN_USEFULNESS;

/// Hard ceiling for `max_open_questions`, whatever the setting says.
pub const MAX_OPEN_CAP: usize = 5;

/// Judge score a persona-led question must reach: only questions the judge
/// calls clearly useful (4 of 5) get through.
const PERSONA_MIN_USEFULNESS: u8 = 4;

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub hold_seconds: i64,
    pub max_open_questions: usize,
    pub min_gap_between_questions_s: i64,
    pub stale_after_s: i64,
    pub warmup_chunks: u32,
    /// Raise questions the way the user's persona would, from the conversation
    /// itself, instead of from the fixed gap types.
    pub persona_led: bool,
    /// Lowest judge usefulness score (1 to 5) that still passes.
    pub min_usefulness: u8,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            hold_seconds: 30,
            max_open_questions: 2,
            min_gap_between_questions_s: 60,
            stale_after_s: 600,
            warmup_chunks: 2,
            persona_led: false,
            min_usefulness: MIN_USEFULNESS,
        }
    }
}

impl PipelineConfig {
    /// Persona-led questions, held to a higher bar: quality over quantity.
    pub fn persona_led() -> Self {
        Self { persona_led: true, min_usefulness: PERSONA_MIN_USEFULNESS, ..Self::default() }
    }
}
