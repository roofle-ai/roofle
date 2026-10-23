//! Replay a transcript through the pipeline against a real model and print a
//! chunk-by-chunk table.
//!
//!   OPENROUTER_API_KEY=... cargo run -p live-agent --example replay -- \
//!       live-agent/fixtures/sample_architecture_review.txt [--goal "..."] [--persona-led]
//!
//! `--persona-led` raises questions the way the app does (from the user's persona,
//! no fixed gap types); without it the older gap-type flow runs, for comparison.
//!
//! Env: OPENROUTER_API_KEY (or OPENAI_API_KEY), ROOFLE_MODEL (all stages,
//! default openai/gpt-4o-mini), ROOFLE_BASE_URL (default OpenRouter). Replies are cached in `<name>.cache.json`: that file is what
//! the CI gate replays, so re-run after any prompt change to re-record.
//! Delete the cache first to force fresh model calls.

use live_agent::notes::CompletionClient;
use live_agent::pipeline::replay::{check, parse_transcript, replay, CachedClient, Labels, Utterance};
use live_agent::pipeline::{PipelineConfig, Speaker, Stages};

struct Http {
    base: String,
    key: String,
    model: String,
}

impl CompletionClient for Http {
    fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        self.post(system, user, None)
    }

    fn complete_json(&self, system: &str, user: &str, schema: &serde_json::Value) -> Result<String, String> {
        self.post(system, user, Some(schema))
    }
}

impl Http {
    fn post(&self, system: &str, user: &str, schema: Option<&serde_json::Value>) -> Result<String, String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [{"role":"system","content":system},{"role":"user","content":user}],
        });
        if let Some(s) = schema {
            body["temperature"] = 0.into();
            body["response_format"] = serde_json::json!({"type":"json_schema","json_schema":{"name":"stage_output","strict":true,"schema":s}});
        }
        let resp: serde_json::Value = ureq::post(&format!("{}/chat/completions", self.base.trim_end_matches('/')))
            .set("Authorization", &format!("Bearer {}", self.key))
            .timeout(std::time::Duration::from_secs(30))
            .send_json(body)
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())?;
        resp["choices"][0]["message"]["content"].as_str().map(str::to_string).ok_or("no content".into())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().expect("usage: replay <transcript.txt> [--goal ..]"));
    let flag = |name: &str| args.iter().position(|a| a == name);
    let labels: Labels = std::fs::read_to_string(path.with_extension("labels.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let goal = flag("--goal").and_then(|i| args.get(i + 1).cloned()).unwrap_or(labels.goal.clone());

    let env = |k: &str| std::env::var(k).ok();
    let http = Http {
        base: env("ROOFLE_BASE_URL").unwrap_or_else(|| "https://openrouter.ai/api/v1".into()),
        key: env("OPENROUTER_API_KEY").or_else(|| env("OPENAI_API_KEY")).unwrap_or_default(),
        model: env("ROOFLE_MODEL").unwrap_or_else(|| "openai/gpt-4o-mini".into()),
    };
    // ponytail: one model for every stage here; the app uses per-stage models.
    let client = CachedClient::open(path.with_extension("cache.json"), Some(&http));

    let text = std::fs::read_to_string(&path).expect("read transcript");
    let utts = parse_transcript(&text).unwrap_or_else(|_| parse_plain(&text));
    let stages = Stages { understand: &client, write: &client, judge: &client };
    let cfg = if flag("--persona-led").is_some() { PipelineConfig::persona_led() } else { PipelineConfig::default() };
    let report = replay(&utts, &goal, &stages, &cfg);

    let mmss = |ms: i64| format!("{:02}:{:02}", ms / 60_000, ms / 1000 % 60);

    println!("QUESTIONS TO ASK ({})", report.surfaced.len());
    if report.surfaced.is_empty() {
        let failed = report.chunks.iter().filter(|c| c.error.is_some()).count();
        if failed > 0 {
            println!("  (none: {failed} chunk(s) failed, see PER CHUNK below)");
        } else {
            println!("  (none: this transcript had no gap worth a question)");
        }
    }
    for (n, s) in report.surfaced.iter().enumerate() {
        println!("  {}. {}\n     raised at {} from the line at {}", n + 1, s.text, mmss(s.opened_ms), mmss(s.source_ts_ms));
    }

    println!("\nPER CHUNK");
    println!("  {:<6}{:<6}{:<10}{}", "chunk", "gaps", "result", "candidate / reason dropped");
    for c in &report.chunks {
        let result = if c.queued.is_some() { "queued" } else if c.error.is_some() { "error" } else { "-" };
        let note = match (&c.error, &c.dropped, &c.candidate) {
            (Some(e), _, _) => e.clone(),
            (_, Some(d), Some(q)) => format!("{q}  [dropped: {d}]"),
            (_, Some(d), None) => format!("[dropped: {d}]"),
            (_, None, Some(q)) => q.clone(),
            _ => String::new(),
        };
        println!("  {:<6}{:<6}{:<10}{}", c.id, c.gaps, result, note);
    }

    let failures = check(&report, &labels);
    if failures.is_empty() {
        println!("\nlabels: ok");
    } else {
        println!("\nlabels FAILED:\n  {}", failures.join("\n  "));
        std::process::exit(1);
    }
}

/// Fallback for a plain transcript with no timestamps: one utterance per
/// non-empty line, 10 s apart, all from the party being questioned.
fn parse_plain(text: &str) -> Vec<Utterance> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, l)| Utterance { ts_ms: i as i64 * 10_000, speaker: Speaker::Presenter, text: l.to_string() })
        .collect()
}
