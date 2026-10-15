//! offline-agent — post-meeting work on a *finished* session.
//!
//! Unlike [`live-agent`] (real-time), this agent runs once the call is over:
//!
//! - [`analysis`]: the user picks a **context** (the kind of conversation); the
//!   LLM returns a structured [`analysis::AnalysisResult`] — scores, reasoning,
//!   strengths and improvements — for the dashboard.
//! - [`recipes`]: one-click drafts (recap email, Slack post) from the notes.
//! - [`action_items`]: owners, tasks and due dates parsed from the notes.

pub mod action_items;
pub mod analysis;
pub mod recipes;
pub mod sections;
pub mod title;

pub use action_items::{parse_action_items, ActionItem};
pub use analysis::{analyse, AnalysisResult};
pub use recipes::{Draft, Recipe};
pub use title::generate_title;
