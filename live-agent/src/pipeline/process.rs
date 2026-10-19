//! One chunk through stages 1-3. Memory is only touched after Stage 1 parses,
//! so a failed chunk never corrupts it; every later failure fails closed.

use crate::notes::CompletionClient;

use super::config::PipelineConfig;
use super::gaps::{pick_best, render_user_prompt, WriteInput};
use super::guards;
use super::memory::{Memory, Question, QuestionState, Speaker};
use super::stages::*;

#[derive(Debug, Clone)]
pub struct Line {
    /// Numeric part of the `T<id>` tag the model cites.
    pub id: u32,
    pub speaker: Speaker,
    pub ts_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: u32,
    pub t_start_ms: i64,
    pub t_end_ms: i64,
    pub lines: Vec<Line>,
}

impl Chunk {
    /// `[T12][Presenter] text` per line, as the prompts expect.
    pub fn render(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                let who = match l.speaker {
                    Speaker::Presenter => "Presenter",
                    Speaker::Other => "Other",
                    Speaker::Unknown => "Unknown",
                };
                format!("[T{}][{who}] {}", l.id, l.text.trim())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn line_of(&self, source_id: &str) -> Option<&Line> {
        let id: u32 = source_id.trim_start_matches('T').parse().ok()?;
        self.lines.iter().find(|l| l.id == id)
    }

    /// Call time of the cited line, else the chunk start.
    fn ts_of(&self, source_id: &str) -> i64 {
        self.line_of(source_id).map_or(self.t_start_ms, |l| l.ts_ms)
    }
}

/// One client per stage so each can use its own model.
pub struct Stages<'a> {
    pub understand: &'a dyn CompletionClient,
    pub write: &'a dyn CompletionClient,
    pub judge: &'a dyn CompletionClient,
}

/// One logged LLM call, for the `pipeline_runs` table.
#[derive(Debug, Clone)]
pub struct StageRun {
    pub stage: &'static str,
    pub prompt_version: &'static str,
    pub input: String,
    pub output: Result<String, String>,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub gaps: usize,
    pub candidate: Option<String>,
    /// Id of the question queued as Pending.
    pub queued: Option<String>,
    /// Why nothing was queued (None when something was, or no gap existed).
    pub dropped: Option<String>,
    /// Questions this chunk answered (dropped if pending, resolved if open).
    pub resolved: Vec<String>,
    pub runs: Vec<StageRun>,
}

fn call<T: for<'de> serde::Deserialize<'de>>(
    out: &mut Outcome,
    client: &dyn CompletionClient,
    stage: &'static str,
    version: &'static str,
    system: &str,
    user: String,
    schema: &serde_json::Value,
) -> Result<T, String> {
    let raw = client.complete_json(system, &user, schema);
    out.runs.push(StageRun { stage, prompt_version: version, input: user, output: raw.clone() });
    parse(&raw?)
}

pub fn process_chunk(
    mem: &mut Memory,
    chunk: &Chunk,
    stages: &Stages,
    cfg: &PipelineConfig,
    now_ms: i64,
) -> Result<Outcome, String> {
    let mut out = Outcome::default();

    // Stage 1: understand.
    let glossary: Vec<String> = mem
        .glossary
        .iter()
        .map(|g| format!("{} (misheard as: {})", g.term, g.heard_as.join(", ")))
        .collect();
    let points: Vec<String> = mem.open_points.iter().map(|p| format!("{}: {}", p.id, p.text)).collect();
    let live: Vec<String> = mem.live_questions().iter().map(|q| format!("{}: {}", q.id, q.text)).collect();
    let recent: Vec<&str> = mem.recent_text.iter().map(String::as_str).collect();
    let persona = mem.persona.describe(&mem.goal);
    let user = format!(
        "MEETING TYPE: {:?}\nABOUT THE USER:\n{}\n\nGOAL:\n{}\n\nGLOSSARY:\n{}\n\nSUMMARY:\n{}\n\nRECENT text:\n{}\n\nOPEN POINTS:\n{}\n\nQUESTIONS:\n{}\n\nCHUNK:\n{}",
        mem.meeting_type,
        or_none(&persona),
        or_none(&mem.goal),
        or_none(&glossary.join("\n")),
        or_none(&mem.summary),
        or_none(&recent.join("\n")),
        or_none(&points.join("\n")),
        or_none(&live.join("\n")),
        chunk.render(),
    );
    let mut u: Understand = call(&mut out, stages.understand, "understand", UNDERSTAND_VERSION, UNDERSTAND_SYSTEM, user, &understand_schema())?;

    // The judge must not find "the answer" in the chunk that raised the gap, so
    // it sees the memory as it was before this chunk was folded in.
    let prior_summary = mem.summary.clone();
    let prior_text = mem.recent_text.iter().cloned().collect::<Vec<_>>().join("\n");

    // Memory update.
    mem.chunks_seen += 1;
    mem.set_summary(&u.summary);
    mem.push_recent(u.clean_chunk.clone());
    mem.resolve_points(&u.answered_points);
    for qid in &u.answered_questions {
        let to = match mem.question(qid).map(|q| q.state) {
            Some(QuestionState::Pending) => QuestionState::Dismissed,
            Some(QuestionState::Open | QuestionState::Stale) => QuestionState::Resolved,
            _ => continue,
        };
        if mem.transition(qid, to, Some(chunk.id)).is_ok() {
            out.resolved.push(qid.clone());
        }
    }
    // Questions are only ever raised from the other party's words. The model's own
    // `speaker` is not trusted: keep a gap only if the line it cites really came from
    // the Presenter (system audio), never the user's mic or an unknown/uncited line.
    u.gaps.retain(|g| chunk.line_of(&g.source_id).is_some_and(|l| l.speaker == Speaker::Presenter));
    for g in &mut u.gaps {
        g.speaker = Speaker::Presenter;
    }
    out.gaps = u.gaps.len();
    for g in &u.gaps {
        mem.add_point(&g.quote, g.gap_type, g.speaker, chunk.id, chunk.ts_of(&g.source_id));
    }

    // Silence is the default: no gap, no more calls.
    let Some(gap) = pick_best(&u.gaps, |g| g.gap_type) else { return Ok(out) };
    if mem.chunks_seen <= cfg.warmup_chunks && !mem.goal_user_set() {
        out.dropped = Some("warm-up".into());
        return Ok(out);
    }

    // Stage 2: write.
    let shown: Vec<String> = mem.recent_question_texts().iter().map(|q| q.to_string()).collect();
    let user = render_user_prompt(&WriteInput {
        gap_type: gap.gap_type,
        key_phrase: &gap.key_phrase,
        quote: &gap.quote,
        note: &gap.note,
        speaker: &format!("{:?}", gap.speaker),
        meeting_type: &format!("{:?}", mem.meeting_type),
        persona: &persona,
        goal: &mem.goal,
        summary: &mem.summary,
        shown: &shown,
        glossary: &glossary.join("\n"),
        chunk_id: chunk.id,
    });
    let text = match call::<Written>(&mut out, stages.write, "write", WRITE_VERSION, WRITE_SYSTEM, user, &write_schema()) {
        Ok(Written { question: Some(q), .. }) if !q.trim().is_empty() => q.trim().to_string(),
        Ok(w) => return Ok(drop_with(out, format!("write: no question [{}]", w.angle.chars().take(120).collect::<String>()))),
        Err(e) => return Ok(drop_with(out, format!("write failed: {e}"))),
    };
    out.candidate = Some(text.clone());

    // Code guards.
    if let Err(reason) = guards::check(&text, gap.speaker, &mem.glossary, &mem.recent_question_texts()) {
        return Ok(drop_with(out, reason));
    }

    // Stage 3: judge (fail closed).
    let existing: Vec<&str> = mem.recent_question_texts();
    let user = format!(
        "MEETING TYPE: {:?}\nABOUT THE USER:\n{}\n\nGOAL:\n{}\n\nPRIOR SUMMARY (before this chunk):\n{}\n\nPRIOR TEXT:\n{}\n\nCURRENT CHUNK (the gap was spotted here):\n{}\n\nOPEN POINTS:\n{}\n\nEXISTING QUESTIONS:\n{}\n\nCANDIDATE QUESTION:\n{}",
        mem.meeting_type,
        or_none(&persona),
        or_none(&mem.goal),
        or_none(&prior_summary),
        or_none(&prior_text),
        u.clean_chunk,
        or_none(&points.join("\n")),
        or_none(&existing.join("\n")),
        text,
    );
    let verdict = match call::<Verdict>(&mut out, stages.judge, "judge", JUDGE_VERSION, JUDGE_SYSTEM, user, &judge_schema()) {
        Ok(v) => v,
        Err(e) => return Ok(drop_with(out, format!("judge failed: {e}"))),
    };
    let need_goal = !mem.goal.trim().is_empty() && mem.goal_inferred;
    if !verdict.passes(need_goal) {
        let evidence: String = verdict.evidence.chars().take(120).collect();
        return Ok(drop_with(out, format!("judge: {} [{}]", verdict.failure(need_goal), evidence)));
    }

    // Rate limit: one new question per window unless the judge flags high impact.
    let last = mem.questions.iter().map(|q| q.created_at_ms).max();
    if !verdict.high_impact
        && last.is_some_and(|t| now_ms - t < cfg.min_gap_between_questions_s * 1000)
    {
        return Ok(drop_with(out, "rate limit".into()));
    }

    let id = mem.next_id('Q');
    mem.questions.push(Question {
        id: id.clone(),
        text,
        gap_type: gap.gap_type,
        source_ts_ms: chunk.ts_of(&gap.source_id),
        source_chunk: chunk.id,
        state: QuestionState::Pending,
        created_at_ms: now_ms,
        ready_at_ms: now_ms + cfg.hold_seconds * 1000,
        opened_at_ms: None,
        resolved_by_chunk: None,
        outcome_change: verdict.outcome_change,
    });
    out.queued = Some(id);
    Ok(out)
}

fn drop_with(mut out: Outcome, reason: String) -> Outcome {
    out.dropped = Some(reason);
    out
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::pipeline::memory::question;

    /// Replays scripted replies in call order and counts calls.
    struct Script(RefCell<VecDeque<Result<String, String>>>, RefCell<usize>);
    impl Script {
        fn new(replies: &[Result<&str, &str>]) -> Self {
            let q = replies.iter().map(|r| r.map(String::from).map_err(String::from)).collect();
            Self(RefCell::new(q), RefCell::new(0))
        }
        fn calls(&self) -> usize { *self.1.borrow() }
    }
    impl CompletionClient for Script {
        fn complete(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
        fn complete_json(&self, _: &str, _: &str, _: &serde_json::Value) -> Result<String, String> {
            *self.1.borrow_mut() += 1;
            self.0.borrow_mut().pop_front().expect("unexpected extra LLM call")
        }
    }

    fn chunk() -> Chunk {
        Chunk {
            id: 3, t_start_ms: 1_000, t_end_ms: 61_000,
            lines: vec![Line { id: 12, speaker: Speaker::Presenter, ts_ms: 42_000, text: "We deploy with Glue then trigger dbt".into() }],
        }
    }

    const NO_GAP: &str = r#"{"clean_chunk":"c","summary":"s","answered_points":[],"answered_questions":[],"gaps":[]}"#;
    const ONE_GAP: &str = r#"{"clean_chunk":"c","summary":"s","answered_points":[],"answered_questions":[],"gaps":[{"type":"PLAN_NO_FALLBACK","quote":"trigger dbt","speaker":"presenter","source_id":"T12","note":"no failure handling"}]}"#;
    const Q: &str = r#"{"question":"What happens if the Glue job succeeds but the dbt trigger call fails?"}"#;
    const PASS: &str = r#"{"answered":false,"evidence":"","duplicate":false,"generic":false,"detail_only":false,"goal_relevant":true,"high_impact":false,"outcome_change":"A manual rerun answer would justify Step Functions"}"#;
    const LOW_VALUE: &str = r#"{"answered":false,"evidence":"","duplicate":false,"generic":false,"detail_only":false,"goal_relevant":true,"high_impact":false,"outcome_change":"A manual rerun answer would justify Step Functions","usefulness":2}"#;
    const GENERIC: &str = r#"{"answered":false,"evidence":"","duplicate":false,"generic":true,"detail_only":false,"goal_relevant":true,"high_impact":false,"outcome_change":"A manual rerun answer would justify Step Functions"}"#;

    fn run(mem: &mut Memory, replies: &[Result<&str, &str>]) -> (Result<Outcome, String>, usize) {
        let s = Script::new(replies);
        let stages = Stages { understand: &s, write: &s, judge: &s };
        let r = process_chunk(mem, &chunk(), &stages, &PipelineConfig::default(), 100_000);
        (r, s.calls())
    }

    fn user_goal_memory() -> Memory {
        let mut m = Memory::new("s");
        m.goal = "Stay on Glue or move?".into();
        m
    }

    #[test]
    fn empty_chunk_costs_one_call() {
        let mut m = user_goal_memory();
        let (r, calls) = run(&mut m, &[Ok(NO_GAP)]);
        assert_eq!(calls, 1);
        assert!(r.unwrap().queued.is_none());
        assert_eq!(m.summary, "s");
    }

    #[test]
    fn good_gap_is_queued_pending_with_source_time() {
        let mut m = user_goal_memory();
        let (r, calls) = run(&mut m, &[Ok(ONE_GAP), Ok(Q), Ok(PASS)]);
        assert_eq!(calls, 3);
        let out = r.unwrap();
        let q = m.question(&out.queued.unwrap()).unwrap();
        assert_eq!(q.state, QuestionState::Pending);
        assert_eq!(q.source_ts_ms, 42_000);
        assert_eq!(q.ready_at_ms, 130_000);
    }

    #[test]
    fn judge_failure_and_rejection_fail_closed() {
        let mut m = user_goal_memory();
        let (r, _) = run(&mut m, &[Ok(ONE_GAP), Ok(Q), Err("timeout")]);
        assert!(r.unwrap().dropped.unwrap().starts_with("judge failed"));
        let (r, _) = run(&mut m, &[Ok(ONE_GAP), Ok(Q), Ok(GENERIC)]);
        assert!(r.unwrap().dropped.unwrap().starts_with("judge: generic"));
        assert!(m.questions.is_empty());
        let (r, _) = run(&mut m, &[Ok(ONE_GAP), Ok(Q), Ok(LOW_VALUE)]);
        assert!(r.unwrap().dropped.unwrap().starts_with("judge: low usefulness"));
    }

    #[test]
    fn judge_sees_prior_memory_not_the_chunk_that_raised_the_gap() {
        struct Spy(RefCell<Vec<String>>);
        impl CompletionClient for Spy {
            fn complete(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
            fn complete_json(&self, system: &str, user: &str, _: &serde_json::Value) -> Result<String, String> {
                if system.starts_with("You judge") { self.0.borrow_mut().push(user.to_string()); }
                Ok(if system.starts_with("You listen to one chunk") {
                    ONE_GAP.replace(r#""summary":"s""#, r#""summary":"NEWSUMMARY""#)
                } else if system.starts_with("You write ONE") { Q.into() } else { PASS.into() })
            }
        }
        let mut m = user_goal_memory();
        m.summary = "OLDSUMMARY".into();
        let spy = Spy(RefCell::default());
        let stages = Stages { understand: &spy, write: &spy, judge: &spy };
        process_chunk(&mut m, &chunk(), &stages, &PipelineConfig::default(), 100_000).unwrap();
        let judged = spy.0.borrow()[0].clone();
        assert!(judged.contains("OLDSUMMARY") && !judged.contains("NEWSUMMARY"));
        assert!(judged.contains("We deploy with Glue") || judged.contains("CURRENT CHUNK"));
        assert_eq!(m.summary, "NEWSUMMARY");
    }

    #[test]
    fn stage_one_failure_leaves_memory_untouched() {
        let mut m = user_goal_memory();
        let (r, _) = run(&mut m, &[Err("500")]);
        assert!(r.is_err());
        assert_eq!(m.chunks_seen, 0);
        assert!(m.summary.is_empty());
    }

    #[test]
    fn answered_while_pending_never_opens() {
        let mut m = user_goal_memory();
        m.questions.push(question("Q1", QuestionState::Pending));
        let reply = r#"{"clean_chunk":"c","summary":"s","answered_points":[],"answered_questions":["Q1","Q99"],"gaps":[]}"#;
        let (r, _) = run(&mut m, &[Ok(reply)]);
        assert_eq!(r.unwrap().resolved, vec!["Q1"]);
        assert_eq!(m.question("Q1").unwrap().state, QuestionState::Dismissed);
    }

    #[test]
    fn gaps_cited_from_the_mic_are_ignored() {
        let mut m = user_goal_memory();
        let mut c = chunk();
        c.lines.push(Line { id: 13, speaker: Speaker::Other, ts_ms: 50_000, text: "I think the dbt trigger might fail".into() });
        let s = Script::new(&[Ok(&ONE_GAP.replace("T12", "T13"))]);
        let stages = Stages { understand: &s, write: &s, judge: &s };
        let out = process_chunk(&mut m, &c, &stages, &PipelineConfig::default(), 100_000).unwrap();
        assert_eq!((s.calls(), out.gaps, m.open_points.len()), (1, 0, 0));
    }

    #[test]
    fn write_prompt_carries_persona_and_null_is_a_clean_drop() {
        struct Spy(RefCell<String>);
        impl CompletionClient for Spy {
            fn complete(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
            fn complete_json(&self, system: &str, user: &str, _: &serde_json::Value) -> Result<String, String> {
                if system.starts_with("You write ONE") {
                    *self.0.borrow_mut() = user.to_string();
                    return Ok(r#"{"angle":"nothing changes","question":null}"#.into());
                }
                Ok(ONE_GAP.into())
            }
        }
        let mut m = user_goal_memory();
        m.meeting_type = crate::pipeline::MeetingType::Interview;
        m.persona = roofle_core::persona::PersonaProfile { role: "Tech Lead".into(), ..Default::default() };
        let spy = Spy(RefCell::default());
        let stages = Stages { understand: &spy, write: &spy, judge: &spy };
        let out = process_chunk(&mut m, &chunk(), &stages, &PipelineConfig::default(), 100_000).unwrap();
        let sent = spy.0.borrow().clone();
        assert!(sent.contains("MEETING TYPE: Interview") && sent.contains("I am a Tech Lead."));
        assert!(out.dropped.unwrap().starts_with("write: no question"));
    }

    #[test]
    fn warmup_blocks_questions_without_a_user_goal() {
        let mut m = Memory::new("s");
        let (r, calls) = run(&mut m, &[Ok(ONE_GAP)]);
        assert_eq!(calls, 1);
        assert_eq!(r.unwrap().dropped.unwrap(), "warm-up");
    }
}
