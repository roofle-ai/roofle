//! High-signal follow-up question pipeline: Understand → Write → Judge →
//! Hold/Release. Pure logic over a [`Memory`]; the caller owns threading,
//! storage and UI events.

pub mod config;
pub mod gaps;
pub mod guards;
pub mod memory;
pub mod process;
pub mod release;
pub mod replay;
pub mod session;
pub mod stages;

pub use config::PipelineConfig;
pub use memory::{Memory, MeetingType, Question, QuestionState, Speaker};
pub use process::{process_chunk, Chunk, Line, Outcome, StageRun, Stages};
pub use release::{tick, Released};
pub use session::{Feedback, Session, UiEvent};
