//! offline-agent — post-meeting self-assessment.
//!
//! Unlike [`live-agent`] (real-time), this agent runs once on a *finished*
//! transcript. The user picks a **context** (the kind of conversation); the agent calls the
//! LLM and returns a structured [`analysis::AnalysisResult`] — scores, reasoning,
//! strengths, improvements and action items — for the dashboard.

pub mod analysis;
pub mod title;

pub use analysis::{analyse, AnalysisResult};
pub use title::generate_title;
