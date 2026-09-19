//! live-agent — real-time meeting analysis.
//!
//! Slice 1 ships the [`ParagraphWatcher`]: a pure, per-session state machine that
//! consumes finalized transcript utterances, accumulates characters, and emits a
//! [`ParagraphBoundary`] each time `char_limit` (default 200) *new* characters
//! have arrived since the last boundary. Each boundary carries both the previous
//! paragraph and the new chunk, giving a future AI agent the context it needs
//! (previous + new) for a critical analysis.
//!
//! Boundaries are persisted through [`ParagraphStore`] (implemented by core's
//! single-writer `SqliteStorage`), so the live-agent owns the domain logic while
//! core owns the one DB connection. When transcription stops, [`flush`] persists
//! the full remaining paragraph even if it is below `char_limit`.

pub mod challenge;
pub mod notes;
pub mod watcher;

pub use challenge::agent_challenges;
pub use notes::{agent_notes, log_step, logging_enabled, CompletionClient};
pub use watcher::{persist_boundary, ParagraphBoundary, ParagraphWatcher, DEFAULT_CHAR_LIMIT};
