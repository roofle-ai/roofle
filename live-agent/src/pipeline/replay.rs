//! Replay harness: run a saved transcript through the pipeline in simulated
//! time, then check what surfaced against a label file. Drives the
//! `replay` example and the fixture tests; no sleeping, no network.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::notes::CompletionClient;
use crate::watcher::ParagraphWatcher;

use super::config::PipelineConfig;
use super::memory::{Memory, Speaker};
use super::process::{Chunk, Line, Stages};
use super::session::{Session, UiEvent};

/// Flush only a tail this long, as in the live engine.
const MIN_FLUSH_CHARS: usize = 150;
const TICK_MS: i64 = 5_000;

#[derive(Debug, Clone)]
pub struct Utterance {
    pub ts_ms: i64,
    pub speaker: Speaker,
    pub text: String,
}

/// Parse `MM:SS Presenter|Other: text` lines; blank lines and `#` comments skipped.
pub fn parse_transcript(s: &str) -> Result<Vec<Utterance>, String> {
    let mut out = Vec::new();
    for (n, line) in s.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || format!("line {}: expected `MM:SS Presenter|Other: text`", n + 1);
        let (stamp, rest) = line.split_once(' ').ok_or_else(bad)?;
        let (mm, ss) = stamp.split_once(':').ok_or_else(bad)?;
        let ts_ms = (mm.parse::<i64>().map_err(|_| bad())? * 60 + ss.parse::<i64>().map_err(|_| bad())?) * 1000;
        let (who, text) = rest.split_once(':').ok_or_else(bad)?;
        let speaker = match who.trim().to_lowercase().as_str() {
            "presenter" => Speaker::Presenter,
            "other" => Speaker::Other,
            _ => Speaker::Unknown,
        };
        out.push(Utterance { ts_ms, speaker, text: text.trim().to_string() });
    }
    Ok(out)
}

/// Split into chunks with the live watcher's rules (500 chars or 60 s).
pub fn chunk(utts: &[Utterance]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut w = ParagraphWatcher::new(crate::watcher::DEFAULT_CHAR_LIMIT);
    let mut lines: Vec<Line> = Vec::new();
    let mut next_line = 1;

    let close = |lines: &mut Vec<Line>, chunks: &mut Vec<Chunk>| {
        if lines.is_empty() {
            return;
        }
        chunks.push(Chunk {
            id: chunks.len() as u32 + 1,
            t_start_ms: lines[0].ts_ms,
            t_end_ms: lines[lines.len() - 1].ts_ms,
            lines: std::mem::take(lines),
        });
    };

    for u in utts {
        // Silence past the time limit closes the chunk before this line.
        if w.due(u.ts_ms) {
            close(&mut lines, &mut chunks);
            w.reset();
        }
        lines.push(Line { id: next_line, speaker: u.speaker, ts_ms: u.ts_ms, text: u.text.clone() });
        next_line += 1;
        if w.push_at(&u.text, u.ts_ms) {
            close(&mut lines, &mut chunks);
            w.reset();
        }
    }
    if w.pending_chars() >= MIN_FLUSH_CHARS {
        close(&mut lines, &mut chunks);
    }
    chunks
}

#[derive(Debug)]
pub struct ChunkReport {
    pub id: u32,
    pub gaps: usize,
    pub candidate: Option<String>,
    pub queued: Option<String>,
    pub dropped: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Surfaced {
    pub text: String,
    pub opened_ms: i64,
    pub source_ts_ms: i64,
}

#[derive(Debug, Default)]
pub struct Report {
    pub chunks: Vec<ChunkReport>,
    pub surfaced: Vec<Surfaced>,
    pub duration_ms: i64,
}

/// Run the whole transcript. A chunk whose LLM call fails is reported and skipped.
pub fn replay(utts: &[Utterance], goal: &str, stages: &Stages, cfg: &PipelineConfig) -> Report {
    let mut mem = Memory::new("replay");
    mem.goal = goal.to_string();
    let mut session = Session::new(mem);
    session.cfg = cfg.clone();

    let chunks = chunk(utts);
    let mut report = Report { duration_ms: utts.last().map_or(0, |u| u.ts_ms), ..Report::default() };
    for (i, c) in chunks.iter().enumerate() {
        let now = c.t_end_ms;
        let mut r = ChunkReport { id: c.id, gaps: 0, candidate: None, queued: None, dropped: None, error: None };
        match session.on_chunk(c, stages, now) {
            Ok((o, _)) => {
                r.gaps = o.gaps;
                r.candidate = o.candidate;
                r.queued = o.queued;
                r.dropped = o.dropped;
            }
            Err(e) => r.error = Some(e),
        }
        report.chunks.push(r);

        // Tick through simulated time until the next chunk (or past the last hold).
        let until = chunks.get(i + 1).map_or(now + (cfg.hold_seconds + 10) * 1000, |n| n.t_end_ms);
        let mut t = now;
        while t <= until {
            for ev in session.on_tick(t) {
                if let UiEvent::Opened { id } = ev {
                    if let Some(q) = session.mem.question(&id) {
                        report.surfaced.push(Surfaced { text: q.text.clone(), opened_ms: t, source_ts_ms: q.source_ts_ms });
                    }
                }
            }
            t += TICK_MS;
        }
    }
    report
}

/// Expectations for one fixture, from `<name>.labels.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Labels {
    pub goal: String,
    /// Each group is satisfied if any alternative (space-separated keywords,
    /// all required) appears in some surfaced question.
    pub must_surface: Vec<Vec<String>>,
    /// Substrings (case-insensitive) that must never appear in a surfaced question.
    pub must_not_surface: Vec<String>,
    pub max_questions_per_10_min: Option<usize>,
}

/// Failures against the labels; empty means the fixture passes.
pub fn check(report: &Report, labels: &Labels) -> Vec<String> {
    let texts: Vec<String> = report.surfaced.iter().map(|s| s.text.to_lowercase()).collect();
    let mut failures = Vec::new();

    for group in &labels.must_surface {
        let hit = group.iter().any(|alt| {
            let words: Vec<String> = alt.to_lowercase().split_whitespace().map(String::from).collect();
            texts.iter().any(|t| words.iter().all(|w| t.contains(w.as_str())))
        });
        if !hit {
            failures.push(format!("missing: none of {group:?} surfaced"));
        }
    }
    for bad in &labels.must_not_surface {
        if let Some(t) = texts.iter().find(|t| t.contains(&bad.to_lowercase())) {
            failures.push(format!("forbidden {bad:?} surfaced in: {t}"));
        }
    }
    if let Some(max) = labels.max_questions_per_10_min {
        let live = report.surfaced.len();
        let allowed = max.max(((max as i64 * report.duration_ms + 599_999) / 600_000) as usize);
        if live > allowed {
            failures.push(format!("{live} questions surfaced, cap {allowed}"));
        }
    }
    failures
}

fn fnv(system: &str, user: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in system.bytes().chain([1]).chain(user.bytes()) {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Answers from a recorded `<name>.cache.json`. With an `inner` client it
/// records misses (and saves the file); without one a miss is an error, which
/// keeps CI deterministic.
pub struct CachedClient<'a> {
    path: PathBuf,
    map: RefCell<BTreeMap<String, String>>,
    inner: Option<&'a dyn CompletionClient>,
}

impl<'a> CachedClient<'a> {
    pub fn open(path: PathBuf, inner: Option<&'a dyn CompletionClient>) -> Self {
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, map: RefCell::new(map), inner }
    }
}

impl CompletionClient for CachedClient<'_> {
    fn complete(&self, _: &str, _: &str) -> Result<String, String> {
        Err("replay uses structured calls only".into())
    }

    fn complete_json(&self, system: &str, user: &str, schema: &serde_json::Value) -> Result<String, String> {
        let key = fnv(system, user);
        if let Some(hit) = self.map.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let inner = self.inner.ok_or_else(|| format!("cache miss {key}: re-record this fixture"))?;
        let reply = inner.complete_json(system, user, schema)?;
        self.map.borrow_mut().insert(key, reply.clone());
        let json = serde_json::to_string_pretty(&*self.map.borrow()).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, json).map_err(|e| format!("write cache: {e}"))?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCRIPT: &str = "\
# a comment
00:05 Presenter: We will deploy with Glue and then call the dbt trigger.
00:20 Other: Sounds fine, the migration should be cheaper than what we run today for sure.
01:10 Presenter: Next topic then.
03:00 Other: Thanks everyone.";

    #[test]
    fn parses_and_chunks_by_time_limit() {
        let u = parse_transcript(TRANSCRIPT).unwrap();
        assert_eq!(u.len(), 4);
        assert_eq!(u[1].speaker, Speaker::Other);
        // 00:05 + 00:20 close at the 01:10 line (>60 s after 00:05); the short
        // tail under 150 chars is dropped at the end.
        let c = chunk(&u);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].lines.len(), 2);
        assert_eq!(c[0].t_end_ms, 20_000);
        assert!(parse_transcript("garbage").is_err());
    }

    struct Fake;
    impl CompletionClient for Fake {
        fn complete(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
        fn complete_json(&self, system: &str, _: &str, _: &serde_json::Value) -> Result<String, String> {
            Ok(if system.starts_with("You read one chunk") {
                r#"{"clean_chunk":"c","summary":"s","answered_points":[],"answered_questions":[],"gaps":[{"type":"NO_NUMBER","quote":"cheaper","speaker":"other","source_id":"T2","note":"n"}]}"#.into()
            } else if system.starts_with("You write ONE") {
                r#"{"question":"What is the actual annual cost of the migration versus today?"}"#.into()

            } else {
                r#"{"answered":false,"evidence":"","duplicate":false,"generic":false,"detail_only":false,"goal_relevant":true,"high_impact":true,"outcome_change":"A higher cost would flip the decision"}"#.into()
            })
        }
    }

    #[test]
    fn replay_surfaces_after_hold_and_checks_labels() {
        let u = parse_transcript(TRANSCRIPT).unwrap();
        let stages = Stages { understand: &Fake, write: &Fake, judge: &Fake };
        let r = replay(&u, "decide on migration", &stages, &PipelineConfig::default());
        assert_eq!(r.surfaced.len(), 1);
        // Queued at 00:20, released only after the 30 s hold.
        assert!(r.surfaced[0].opened_ms >= 50_000);
        assert_eq!(r.surfaced[0].source_ts_ms, 20_000);

        let ok: Labels = serde_json::from_str(r#"{"must_surface":[["annual cost","price"]],"must_not_surface":["what specific"],"max_questions_per_10_min":2}"#).unwrap();
        assert!(check(&r, &ok).is_empty());
        let bad: Labels = serde_json::from_str(r#"{"must_surface":[["skill gap"]],"must_not_surface":["annual cost"]}"#).unwrap();
        assert_eq!(check(&r, &bad).len(), 2);
    }

    #[test]
    fn cache_replays_and_errors_on_miss() {
        let dir = std::env::temp_dir().join(format!("roofle-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.cache.json");
        let _ = std::fs::remove_file(&path);
        let schema = serde_json::json!({});
        let rec = CachedClient::open(path.clone(), Some(&Fake));
        let reply = rec.complete_json("You write ONE x", "u", &schema).unwrap();
        let replay = CachedClient::open(path, None);
        assert_eq!(replay.complete_json("You write ONE x", "u", &schema).unwrap(), reply);
        assert!(replay.complete_json("You write ONE x", "other", &schema).unwrap_err().contains("cache miss"));
    }
}
