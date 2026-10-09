//! One session's pipeline state: memory, config and circuit breaker, with no
//! I/O. The app's session worker feeds it chunks and ticks and persists the
//! events it returns; keeping it pure makes isolation testable.

use crate::breaker::Breaker;

use super::config::PipelineConfig;
use super::memory::{Memory, QuestionState};
use super::process::{process_chunk, Chunk, Outcome, Stages};
use super::release::{self, Released};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
    Asked,
    Dismissed,
}

impl Feedback {
    pub fn as_str(self) -> &'static str {
        match self {
            Feedback::Asked => "asked",
            Feedback::Dismissed => "dismissed",
        }
    }
}

/// What changed for the UI and storage.
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Opened { id: String },
    Resolved { id: String, by_chunk: u32 },
    Stale { id: String },
}

pub struct Session {
    pub mem: Memory,
    pub cfg: PipelineConfig,
    breaker: Breaker,
}

impl Session {
    pub fn new(mem: Memory) -> Self {
        Self { mem, cfg: PipelineConfig::default(), breaker: Breaker::default() }
    }

    /// Whether the breaker is open (provider failing); the UI shows "degraded".
    pub fn degraded(&self, now_ms: i64) -> bool {
        !self.breaker.allows(now_ms)
    }

    /// Run one chunk. `Err` leaves memory untouched; the chunk is skipped.
    pub fn on_chunk(
        &mut self,
        chunk: &Chunk,
        stages: &Stages,
        now_ms: i64,
    ) -> Result<(Outcome, Vec<UiEvent>), String> {
        if !self.breaker.allows(now_ms) {
            return Err("paused after repeated LLM failures".into());
        }
        let outcome = match process_chunk(&mut self.mem, chunk, stages, &self.cfg, now_ms) {
            Ok(o) => o,
            Err(e) => {
                self.breaker.failure(now_ms);
                return Err(e);
            }
        };
        self.breaker.success();
        // Pending questions answered before surfacing are dropped silently.
        let events = outcome
            .resolved
            .iter()
            .filter(|id| self.mem.question(id).is_some_and(|q| q.state == QuestionState::Resolved))
            .map(|id| UiEvent::Resolved { id: id.clone(), by_chunk: chunk.id })
            .collect();
        Ok((outcome, events))
    }

    /// Release held questions and age open ones. Call every few seconds.
    pub fn on_tick(&mut self, now_ms: i64) -> Vec<UiEvent> {
        release::tick(&mut self.mem, now_ms, &self.cfg)
            .into_iter()
            .map(|r| match r {
                Released::Opened(id) => UiEvent::Opened { id },
                Released::Stale(id) => UiEvent::Stale { id },
            })
            .collect()
    }

    /// Apply the user's feedback. Asked/Dismissed take the question off the
    /// list.
    pub fn on_feedback(&mut self, id: &str, kind: Feedback) {
        let to = match kind {
            Feedback::Asked => QuestionState::Asked,
            Feedback::Dismissed => QuestionState::Dismissed,
        };
        let _ = self.mem.transition(id, to, None);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::notes::CompletionClient;
    use crate::pipeline::memory::Speaker;
    use crate::pipeline::process::Line;

    /// Answers every stage by reading a keyword from its prompt, so two
    /// sessions can run against different "transcripts".
    struct Fake { topic: &'static str, log: RefCell<VecDeque<String>> }
    impl CompletionClient for Fake {
        fn complete(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
        fn complete_json(&self, system: &str, user: &str, _: &serde_json::Value) -> Result<String, String> {
            self.log.borrow_mut().push_back(user.to_string());
            let t = self.topic;
            Ok(if system.starts_with("You listen to one chunk") {
                format!(r#"{{"clean_chunk":"{t} chunk","summary":"{t} summary","answered_points":[],"answered_questions":[],"gaps":[{{"type":"NO_NUMBER","quote":"{t} cost","speaker":"presenter","source_id":"T1","note":"n"}}]}}"#)
            } else if system.starts_with("You write ONE") {
                format!(r#"{{"question":"What is the actual annual {t} cost at our current volume?"}}"#)
            } else {
                r#"{"answered":false,"evidence":"","duplicate":false,"generic":false,"detail_only":false,"goal_relevant":true,"high_impact":true,"outcome_change":"A high number would flip the decision"}"#.to_string()
            })
        }
    }

    fn chunk(text: &str) -> Chunk {
        Chunk { id: 1, t_start_ms: 0, t_end_ms: 1, lines: vec![Line { id: 1, speaker: Speaker::Presenter, ts_ms: 5, text: text.into() }] }
    }

    fn run_session(topic: &'static str) -> (Session, Fake) {
        let mut mem = Memory::new(topic);
        mem.goal = "decide".into();
        (Session::new(mem), Fake { topic, log: RefCell::default() })
    }

    #[test]
    fn two_sessions_never_share_state() {
        let (mut a, fa) = run_session("glue");
        let (mut b, fb) = run_session("sales");
        for (s, f, text) in [(&mut a, &fa, "glue talk"), (&mut b, &fb, "sales talk")] {
            let stages = Stages { understand: f, write: f, judge: f };
            let (out, _) = s.on_chunk(&chunk(text), &stages, 0).unwrap();
            assert!(out.queued.is_some());
            assert_eq!(s.on_tick(75_000), vec![UiEvent::Opened { id: "Q2".into() }]);
        }
        let dump = |s: &Session| serde_json::to_string(&s.mem).unwrap();
        assert!(dump(&a).contains("glue") && !dump(&a).contains("sales"));
        assert!(dump(&b).contains("sales") && !dump(&b).contains("glue"));
        assert!(fa.log.borrow().iter().all(|p| !p.contains("sales")));
        assert!(fb.log.borrow().iter().all(|p| !p.contains("glue")));
    }

    #[test]
    fn breaker_pauses_after_repeated_failures() {
        struct Down;
        impl CompletionClient for Down {
            fn complete(&self, _: &str, _: &str) -> Result<String, String> { Err("down".into()) }
        }
        let mut s = Session::new(Memory::new("s"));
        let stages = Stages { understand: &Down, write: &Down, judge: &Down };
        for _ in 0..5 {
            assert_eq!(s.on_chunk(&chunk("x"), &stages, 0).unwrap_err(), "down");
        }
        assert!(s.degraded(1));
        assert!(s.on_chunk(&chunk("x"), &stages, 1).unwrap_err().contains("paused"));
        assert!(!s.degraded(30_000));
    }

    #[test]
    fn answered_open_question_emits_resolved() {
        let (mut s, f) = run_session("glue");
        let stages = Stages { understand: &f, write: &f, judge: &f };
        s.on_chunk(&chunk("a"), &stages, 0).unwrap();
        s.on_tick(75_000);
        struct Answers;
        impl CompletionClient for Answers {
            fn complete(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
            fn complete_json(&self, _: &str, _: &str, _: &serde_json::Value) -> Result<String, String> {
                Ok(r#"{"clean_chunk":"c","summary":"s","answered_points":[],"answered_questions":["Q2"],"gaps":[]}"#.into())
            }
        }
        let stages = Stages { understand: &Answers, write: &Answers, judge: &Answers };
        let (_, ev) = s.on_chunk(&chunk("b"), &stages, 80_000).unwrap();
        assert_eq!(ev, vec![UiEvent::Resolved { id: "Q2".into(), by_chunk: 1 }]);
    }
}
