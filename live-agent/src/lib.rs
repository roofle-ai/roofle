//! live-agent — real-time meeting analysis.
//!
//! Ships the AI agents ([`notes`], [`pipeline`], [`recap`], [`figures`]) plus the [`ParagraphWatcher`]:
//! a lightweight boundary detector that tells the engine when to finalize an
//! accumulating paragraph and fold it into the merged `transcript_log`. The
//! watcher is pure state; the engine owns persistence.

pub mod assist;
pub mod breaker;
pub mod figures;
pub mod notes;
pub mod pipeline;
pub mod recap;
pub mod watcher;

pub use notes::{
    agent_notes, log_step, logging_enabled, CompletionClient};
pub use figures::{extract_figures, Figure, KeyFigures};
pub use recap::{recap_recent, Recap, Utterance};
pub use watcher::{ParagraphWatcher, DEFAULT_CHAR_LIMIT};
