//! live-agent — real-time meeting analysis.
//!
//! Ships the AI agents ([`notes`], [`challenge`], [`recap`]) plus the [`ParagraphWatcher`]:
//! a lightweight boundary detector that tells the engine when to finalize an
//! accumulating paragraph and fold it into the merged `transcript_log`. The
//! watcher is pure state; the engine owns persistence.

pub mod challenge;
pub mod notes;
pub mod recap;
pub mod watcher;

pub use challenge::{agent_challenges, ChallengeContext, ChallengeResult};
pub use notes::{
    agent_notes, log_step, logging_enabled, CompletionClient, ConversationKind, KindState,
    NotesOutcome,
};
pub use recap::{recap_recent, Recap, Utterance};
pub use watcher::{ParagraphWatcher, DEFAULT_CHAR_LIMIT};
