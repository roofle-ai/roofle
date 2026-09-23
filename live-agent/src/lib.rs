//! live-agent — real-time meeting analysis.
//!
//! Ships the AI agents ([`notes`], [`challenge`]) plus the [`ParagraphWatcher`]:
//! a lightweight boundary detector that tells the engine when to finalize an
//! accumulating paragraph and fold it into the merged `transcript_log`. The
//! watcher is pure state; the engine owns persistence.

pub mod challenge;
pub mod notes;
pub mod watcher;

pub use challenge::{agent_challenges, ChallengeResult};
pub use notes::{agent_notes, log_step, logging_enabled, CompletionClient};
pub use watcher::{ParagraphWatcher, DEFAULT_CHAR_LIMIT};
