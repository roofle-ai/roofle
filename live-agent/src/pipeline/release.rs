//! Stage 4 (code only): hold new questions, release them after the hold
//! window, and age unanswered ones into stale.

use super::config::PipelineConfig;
use super::memory::{Memory, QuestionState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Released {
    Opened(String),
    Stale(String),
}

/// Advance question lifecycles at `now_ms`. Call every few seconds.
pub fn tick(mem: &mut Memory, now_ms: i64, cfg: &PipelineConfig) -> Vec<Released> {
    let mut events = Vec::new();
    let mut open = mem.open_count();

    for q in mem.questions.iter_mut() {
        match q.state {
            QuestionState::Pending if q.ready_at_ms <= now_ms && open < cfg.max_open_questions => {
                q.state = QuestionState::Open;
                q.opened_at_ms = Some(now_ms);
                open += 1;
                events.push(Released::Opened(q.id.clone()));
            }
            QuestionState::Open
                if q.opened_at_ms.is_some_and(|t| now_ms - t >= cfg.stale_after_s * 1000) =>
            {
                q.state = QuestionState::Stale;
                open -= 1;
                events.push(Released::Stale(q.id.clone()));
            }
            _ => {}
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::memory::question;

    fn pending(id: &str, ready: i64) -> crate::pipeline::memory::Question {
        let mut q = question(id, QuestionState::Pending);
        q.ready_at_ms = ready;
        q
    }

    #[test]
    fn releases_after_hold_up_to_cap() {
        let cfg = PipelineConfig::default();
        let mut m = Memory::new("s");
        m.questions = vec![pending("Q1", 1_000), pending("Q2", 1_000), pending("Q3", 1_000)];
        assert!(tick(&mut m, 999, &cfg).is_empty());
        let ev = tick(&mut m, 1_000, &cfg);
        assert_eq!(ev, vec![Released::Opened("Q1".into()), Released::Opened("Q2".into())]);
        assert_eq!(m.question("Q3").unwrap().state, QuestionState::Pending);
    }

    #[test]
    fn unanswered_open_goes_stale_and_frees_a_slot() {
        let cfg = PipelineConfig::default();
        let mut m = Memory::new("s");
        m.questions = vec![pending("Q1", 0), pending("Q2", 0), pending("Q3", 0)];
        tick(&mut m, 0, &cfg);
        let ev = tick(&mut m, 600_000, &cfg);
        assert_eq!(ev[0], Released::Stale("Q1".into()));
        assert_eq!(m.question("Q1").unwrap().state, QuestionState::Stale);
        // Q3 gets the freed slots on the same tick.
        assert!(ev.contains(&Released::Opened("Q3".into())));
    }
}
