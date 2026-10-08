//! Regression gate: every fixture with a recorded `.cache.json` is replayed
//! deterministically and checked against its labels. Fixtures without a cache
//! are skipped (record one with the `replay` example).

use std::path::Path;

use live_agent::pipeline::replay::{check, parse_transcript, replay, CachedClient, Labels};
use live_agent::pipeline::{PipelineConfig, Stages};

#[test]
fn recorded_fixtures_meet_their_labels() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let cache = path.with_extension("cache.json");
        if !cache.exists() {
            eprintln!("skip {}: no cache.json", path.display());
            continue;
        }
        let labels: Labels = std::fs::read_to_string(path.with_extension("labels.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let utts = parse_transcript(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let client = CachedClient::open(cache, None);
        let stages = Stages { understand: &client, write: &client, judge: &client };
        let report = replay(&utts, &labels.goal, &stages, &PipelineConfig::default());
        failures.extend(check(&report, &labels).into_iter().map(|f| format!("{}: {f}", path.display())));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
