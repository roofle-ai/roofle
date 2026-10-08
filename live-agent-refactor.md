# Roofle: High-Signal Follow-Up Question Pipeline

Technical architecture and implementation plan

---

## 1. Goal and non-goals

### Goal
Every 60 seconds of transcript (or 500 characters, whichever comes first), decide whether there is **one** question worth asking that could change the outcome of the conversation. Most chunks should produce nothing. When a question appears, it should be specific, not already answered, and tied to a decision, plan, or risk.

### Non-goals
- Generating many questions per call (the old system produced 17).
- Scoring the speaker (that is the Self-assessment feature; it stays separate).
- Real-time sub-second latency. A 60-90 second delay before showing a question is deliberate.

### Success metrics

| Metric | Target | How measured |
|---|---|---|
| Questions surfaced per 10 min of call | 0-2 | Log count |
| Already-answered-at-surface rate | < 5% | Manual labelling of 10 calls |
| User-judged useful rate | > 60% | Thumbs up/down or "asked it" flag |
| Empty-chunk rate | > 85% | Log `gaps == []` |
| Duplicate / rephrased questions | 0 | Dedup check in judge |
| Cross-session contamination | 0 | Integration test (see 11.4) |

---

## 2. Problems observed in the current system

These drive the design, and each must have a test.

1. Questions are generated per transcript line, so one chunk yields 4+ questions.
2. Most questions follow "You mentioned X, what specific...", asking for detail already given.
3. Questions are generated without checking whether they were answered earlier or later.
4. Misheard terms ("5-tran", "Firetron", "5 trunks", "fifth trend") leak into questions.
5. Statements are misattributed ("You mentioned monitoring" when the other party said it).
6. The Sales perspective was applied to an architecture review, producing a 0/100 self-assessment.
7. A session showed notes and questions from a different session (state leak).
8. UI states are inconsistent: a card labelled OPEN shows "Resolved"; all timestamps show 00:00.
9. The small model (gpt-4o-mini class) ignores long prompts, inflates numeric scores, and skips silent reasoning.

---

## 3. Design principles

1. **Silence is the default.** The pipeline's normal output is nothing.
2. **One job per LLM call.** Small models classify and rewrite well. They judge "importance" badly.
3. **Closed vocabulary over open judgement.** Gaps come from a fixed list of types, not "is this important?".
4. **Binary checks with evidence, not 0-10 scores.** Scores get inflated by small models.
5. **Memory is data the backend owns.** The model is stateless; the Rust backend keeps and injects state.
6. **Code enforces the rules the model forgets.** Caps, dedup, banned patterns, one question per chunk.
7. **Delay, then verify.** Hold every question and drop it if a later chunk answers it.
8. **Log everything.** Tuning needs the data from every stage.

---

## 4. High-level architecture

```
                   ┌──────────────────────────────────────────────────────────────┐
                   │                      Rust backend (tokio)                    │
                   │                                                              │
 Audio/STT ──►  TranscriptIngest ──►  SessionRegistry ──► SessionActor (1 per session)
 (existing)        (lines + ts)          (HashMap<SessionId, Sender<Msg>>)         │
                   │                                                              │
                   │      ┌───────────────────────────────────────────────────┐   │
                   │      │ SessionActor (owns Memory, no locks)              │   │
                   │      │                                                   │   │
                   │      │  Chunker ─► Pipeline ─► Memory ─► QuestionStore   │   │
                   │      │               │                        ▲          │   │
                   │      │               ▼                        │          │   │
                   │      │      Stage 1 Understand (mini)         │          │   │
                   │      │      Stage 2 Write      (mini)         │          │   │
                   │      │      Stage 3 Judge      (strong)       │          │   │
                   │      │      Stage 4 Hold + Release (timer) ───┘          │   │
                   │      └───────────────────────────────────────────────────┘   │
                   │                │                    │                        │
                   │                ▼                    ▼                        │
                   │         LlmClient (reqwest)    Storage (SQLite/sqlx)         │
                   │                │                    │                        │
                   │                ▼                    ▼                        │
                   │          OpenAI API         memory, questions, logs          │
                   └───────────────────────────┬──────────────────────────────────┘
                                               │ events
                                               ▼
                                        UI (questions panel)
```

### Components

| Component | Responsibility |
|---|---|
| `TranscriptIngest` | Receives transcript lines with timestamps and speaker labels, routes by `session_id` |
| `SessionRegistry` | Creates, looks up and shuts down session actors |
| `SessionActor` | Owns one session's `Memory`. Processes messages serially. Runs the hold-window timer |
| `Chunker` | Buffers lines until 500 chars or 60 s |
| `Pipeline` | Runs stages 1-3 for one chunk |
| `LlmClient` | HTTP calls, structured outputs, retries, timeouts, token accounting |
| `PromptStore` | Versioned prompt templates loaded from files |
| `Storage` | SQLite persistence for memory, questions, and stage logs |
| `QuestionStore` | Lifecycle of each question: pending, open, resolved, stale, dismissed |
| `EventBus` | Sends UI events (question surfaced, resolved, stale) |

---

## 5. Data flow for one chunk

```
lines ─► Chunker (500 chars | 60s) ─► Chunk{id, text, t_start, t_end, speakers}
   │
   ▼
Stage 1 UNDERSTAND (mini, JSON schema)
   in : goal, glossary, summary, open_points, pending+open questions, chunk
   out: clean_chunk, new_summary, answered_points[], answered_questions[], gaps[]
   │
   ├─► update Memory (summary, open_points, auto-resolve questions)
   │
   ├─ gaps empty? ─► END (expected for most chunks)
   │
   ▼
pick best gap by priority (code)
   ▼
Stage 2 WRITE (mini, template per gap type)
   out: question
   ▼
code filters: banned regex, length, speaker-attribution check, duplicate similarity
   ▼
Stage 3 JUDGE (stronger model, binary checks + evidence)
   out: answered, duplicate, generic, detail_only, outcome_change
   ▼
pass? ─► QuestionStore: state = Pending, ready_at = now + 75s
   ▼
timer tick (every 5s):
   for each Pending past ready_at:
       if open_visible < MAX_OPEN ─► state = Open ─► emit UI event
```

Later chunks can mark a Pending question as answered (Stage 1 `answered_questions`), so it never reaches the UI. If it is already Open, it becomes Resolved. An Open question that stays unanswered for 10 minutes becomes Stale.

---

## 6. Memory model

The LLM has no memory. The backend keeps this struct per session and injects the relevant parts into every prompt.

```rust
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Memory {
    pub session_id: String,
    pub goal: String,                 // decision/outcome at stake (user-set or inferred)
    pub goal_inferred: bool,          // inferred goals use a stricter filter
    pub meeting_type: MeetingType,    // Decision, Review, Sales, Status, Interview, Other
    pub glossary: Vec<GlossaryEntry>, // correct term + known mishearings
    pub summary: String,              // rolling, <= 150 words, rewritten each chunk
    pub recent_text: VecDeque<String>,// last 2 clean chunks verbatim
    pub open_points: IndexMap<String, OpenPoint>,   // unresolved claims, capped at 15
    pub questions: IndexMap<String, Question>,      // all questions with state
    pub counter: u64,
    pub chunks_seen: u32,
    pub schema_version: u32,
}

pub struct GlossaryEntry { pub term: String, pub heard_as: Vec<String> }

pub struct OpenPoint {
    pub id: String,           // P12
    pub text: String,         // exact quote
    pub gap_type: GapType,
    pub speaker: Speaker,     // Presenter | Other | Unknown
    pub chunk_id: u32,
    pub source_ts_ms: i64,    // for the UI timestamp
}

pub enum QuestionState { Pending, Open, Resolved, Stale, Dismissed, Asked }

pub struct Question {
    pub id: String,           // Q7
    pub text: String,
    pub gap_type: GapType,
    pub source_ts_ms: i64,    // fixes the 00:00 timestamp bug
    pub state: QuestionState,
    pub created_at_ms: i64,
    pub ready_at_ms: i64,
    pub resolved_by_chunk: Option<u32>,
    pub judge: JudgeVerdict,
}
```

### Memory rules

| Item | Bound | Eviction |
|---|---|---|
| `summary` | 150 words (prompt-enforced, truncated in code at 1,200 chars) | Rewritten each chunk |
| `recent_text` | 2 chunks | FIFO |
| `open_points` | 15 | Oldest unresolved first |
| `questions` shown to model for dedup | last 20 | Older ones remain in DB only |
| `glossary` | 50 | LRU |

### Summary quality guard
The summary is the most likely failure point. The prompt must keep: numbers, owners, dates, constraints, rejected options, decisions, and open questions. In code, after every 10 chunks, run a cheap check that the summary still contains the numbers and names found in the last 10 `clean_chunk`s. If more than 30% are missing, log a warning (and in v2, rebuild the summary from `recent_text` plus stored chunks).

---

## 7. Concurrency model

One tokio task (actor) per session. It owns `Memory` outright.

```rust
pub enum Msg {
    Line { text: String, speaker: Speaker, ts_ms: i64 },
    SetGoal(String),
    SetMeetingType(MeetingType),
    Feedback { question_id: String, kind: FeedbackKind }, // Asked, Dismissed, Useful, NotUseful
    Flush,       // process buffered text now (call ended)
    Shutdown,
}
```

```rust
loop {
    tokio::select! {
        Some(msg) = rx.recv() => handle(msg).await,
        _ = tick.tick() => release_ready_questions().await,   // every 5 s
        else => break,
    }
}
```

Why an actor:
- No `Arc<Mutex<..>>` held across `.await`.
- Chunks are processed strictly in order, so slow LLM calls cannot race and overwrite the summary.
- Sessions are isolated by construction, which prevents the cross-session leak.

### Back-pressure
- Channel capacity 256. If full, drop oldest `Line` messages with a warning, never block the audio thread.
- If a chunk takes longer than 20 s to process and the next chunk is waiting, merge the pending lines into the next chunk rather than building a queue.

---

## 8. Chunking

- Trigger: 500 characters or 60 seconds since the first buffered line, whichever comes first.
- Do not cut mid-sentence if the next line arrives within 3 s. Wait up to 3 s for a sentence end.
- Keep speaker labels and timestamps per line, so each gap can be attributed to a speaker and a source time.
- On call end, `Flush` runs whatever remains if it is above 150 characters.

```rust
pub struct Chunk {
    pub id: u32,
    pub text: String,             // "[T12][Presenter] ..." lines
    pub t_start_ms: i64,
    pub t_end_ms: i64,
    pub lines: Vec<Line>,
}
```

---

## 9. The pipeline stages

All stages use `temperature: 0` and JSON-schema structured outputs, so the response always parses.

### Stage 1: Understand (small model)

Responsibilities:
1. Correct misheard terms using the glossary and context.
2. Rewrite the summary.
3. Mark open points and questions this chunk resolves.
4. List gaps from a closed set of types.

Gap types:

| Type | Meaning | Template question |
|---|---|---|
| `PLAN_NO_FALLBACK` | Plan with no failure handling | "You said X. What happens if step Y fails?" |
| `FUZZY_CRITERIA` | Go/no-go condition stated vaguely | "What has to be true for X to count as done?" |
| `NO_NUMBER` | Cost/size/time/effort claim with no number | "You said X. What is the actual cost/size/time?" |
| `DISMISSED_ALTERNATIVE` | Option rejected or skipped without reason | "Would simpler option Z not be enough?" |
| `NO_OWNER_OR_DATE` | Agreed action with no owner or date | "Who owns X?" |
| `CONTRADICTION` | Conflicts with the summary | "Earlier you said A, now B. Which holds?" |

Priority order for choosing among gaps: `PLAN_NO_FALLBACK` > `FUZZY_CRITERIA` > `NO_NUMBER` > `DISMISSED_ALTERNATIVE` > `NO_OWNER_OR_DATE` > `CONTRADICTION`.

Output schema:

```json
{
  "clean_chunk": "string",
  "summary": "string, max 150 words",
  "answered_points": ["P3"],
  "answered_questions": ["Q2"],
  "gaps": [
    { "type": "PLAN_NO_FALLBACK", "quote": "exact words", "speaker": "presenter|other|unknown", "source_id": "T12", "note": "short" }
  ]
}
```

### Stage 2: Write (small model, one gap)

Input: gap type, quote, speaker, goal, summary. Output: one question under 25 words.
Hard rules in the prompt: never start with "What specific", use the glossary's clean terms, do not write "you mentioned" if the other party said it, ask owner or date but never both.

### Stage 3: Judge (stronger model, only runs on 0-1 candidates per chunk)

```json
{
  "answered": false, "evidence": "",
  "duplicate": false,
  "generic": false,
  "detail_only": false,
  "outcome_change": "one plausible answer that would change the plan"
}
```

Pass condition (code): `!answered && !duplicate && !generic && !detail_only && outcome_change.len() > 15`.

When the goal is inferred (`goal_inferred == true`), add a fifth required check: `goal_relevant == true`.

### Stage 4: Hold and release (code only)

- New questions enter `Pending` with `ready_at = now + 75 s`.
- Every 5 s the actor scans `Pending`. Questions past `ready_at` become `Open` if fewer than `MAX_OPEN` (2) are open.
- Stage 1 of later chunks can drop Pending questions it sees answered.
- Open questions unanswered for 10 minutes become `Stale`.
- At call end, run the end-of-meeting pass (section 10).

---

## 10. End-of-meeting pass

On `Flush` or call end, run one call with a stronger model:

Input: goal, final summary, all questions in state Open or Stale, and the open points.
Output:
- The top 3 unresolved questions, rewritten sharper.
- Up to 2 important questions never raised.
- A short "decisions and unresolved items" list.

This is the real value for a decision meeting. The live questions act as a scratchpad, and the end-of-call list is the final output.

---

## 11. Code-side guards

| Guard | Rule |
|---|---|
| One question per chunk | Stage 2 runs on the single highest-priority gap |
| Banned openings | Regex: `^(what specific|can you (explain|elaborate|tell me more))` |
| Length | 6 to 30 words |
| Single ask | Reject if the question contains " and " joining two question clauses, or more than one "?" |
| Dedup | Normalised text similarity > 0.8 against the last 20 questions (token Jaccard in v1, embeddings in v2) |
| Attribution | If gap `speaker != presenter`, reject questions that start with "You mentioned" or "You said" |
| Garbled terms | Reject if the question contains any `heard_as` entry from the glossary |
| Open cap | `MAX_OPEN = 2` visible open questions |
| Rate limit | At most one new question per 3 min unless the judge's `outcome_change` is flagged high impact |
| Warm-up | No questions in the first 2 chunks unless the goal is user-set |

### 11.4 Required integration tests
- Two sessions run concurrently with different transcripts. Assert no summary text, question, or gap from session A ever appears in session B's memory or UI events.
- A session reloaded from SQLite resumes with identical memory.

---

## 12. LLM client

```rust
pub struct LlmClient { http: reqwest::Client, api_key: String, base_url: String }

impl LlmClient {
    pub async fn json<T: DeserializeOwned>(
        &self, model: &str, prompt: &str, schema: &serde_json::Value,
    ) -> Result<(T, Usage), LlmError>;
}
```

Requirements:
- `temperature: 0`, `response_format` with `json_schema` and `strict: true`.
- Timeout 20 s per call. One retry with jittered backoff on 429, 5xx, timeout. No retry on parse errors (log and skip).
- Capture token usage and latency per call, for cost tracking.
- Model names come from config, never hard-coded (`stage1_model`, `stage2_model`, `judge_model`).
- A circuit breaker: after 5 consecutive failures, pause the pipeline for 30 s and show a quiet status in the UI. A failed chunk never clears memory.

---

## 13. Prompt management

- Prompts live in `prompts/` as versioned files, not inline strings:
  ```
  prompts/
    understand.v1.md
    write.v1.md
    judge.v1.md
    end_of_meeting.v1.md
  ```
- Templates use named placeholders (`{{goal}}`, `{{summary}}`), rendered with `minijinja` or `tera`.
- Each logged run stores the prompt version, so a regression can be traced to the change that caused it.
- Put hard rules first and the few-shot examples last. Keep the Stage 1 prompt under about 700 words for a small model.

### Few-shot examples to include (from real calls)

```
BAD  "What orchestrator options are you considering instead of Airflow?"
     Options were just named; answer would not change the decision.
BAD  "What specific timeframe are you referring to for the switchover?"
     Asks for a date; the real gap is the criteria.
BAD  "What specific changes are you considering for the current pipeline given that it is live?"
     Already answered: monitoring can go in, purge needs table config changes.
GOOD "What happens if the Glue job succeeds but the dbt trigger call fails?"
     PLAN_NO_FALLBACK; answer decides whether Step Functions is justified.
GOOD "What has to be true for the new pipeline to count as ready to switch over?"
     FUZZY_CRITERIA; decides cutover timing.
GOOD "What is the estimated annual cost of Fivetran hybrid versus Glue at our volume?"
     NO_NUMBER; could flip the recommendation.
```

---

## 14. Storage schema (SQLite via sqlx)

```sql
CREATE TABLE sessions (
  session_id   TEXT PRIMARY KEY,
  title        TEXT,
  goal         TEXT,
  meeting_type TEXT,
  created_at   INTEGER NOT NULL,
  ended_at     INTEGER
);

CREATE TABLE session_memory (
  session_id   TEXT PRIMARY KEY REFERENCES sessions(session_id),
  memory_json  TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE transcript_lines (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id   TEXT NOT NULL REFERENCES sessions(session_id),
  ts_ms        INTEGER NOT NULL,
  speaker      TEXT,
  text         TEXT NOT NULL
);
CREATE INDEX idx_lines_session ON transcript_lines(session_id, ts_ms);

CREATE TABLE questions (
  question_id  TEXT NOT NULL,
  session_id   TEXT NOT NULL REFERENCES sessions(session_id),
  text         TEXT NOT NULL,
  gap_type     TEXT,
  state        TEXT NOT NULL,       -- pending|open|resolved|stale|dismissed|asked
  source_ts_ms INTEGER,
  created_at   INTEGER NOT NULL,
  surfaced_at  INTEGER,
  resolved_at  INTEGER,
  feedback     TEXT,                -- useful|not_useful|asked|dismissed
  PRIMARY KEY (session_id, question_id)
);

CREATE TABLE pipeline_runs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id   TEXT NOT NULL,
  chunk_id     INTEGER NOT NULL,
  stage        TEXT NOT NULL,       -- understand|write|judge|end
  model        TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  input_json   TEXT NOT NULL,
  output_json  TEXT,
  error        TEXT,
  tokens_in    INTEGER,
  tokens_out   INTEGER,
  latency_ms   INTEGER,
  created_at   INTEGER NOT NULL
);
CREATE INDEX idx_runs_session ON pipeline_runs(session_id, chunk_id);
```

Rules:
- Every query includes `session_id`. No query reads across sessions.
- `session_memory` is saved after each processed chunk and after each release tick that changes state. Memory is saved only on a consistent state (after the chunk finishes, never mid-call).
- `schema_version` allows migrating old memory JSON when the struct changes.

---

## 15. UI contract

Events from backend to UI (keyed by `session_id`):

```json
{ "type": "question_opened",   "session_id": "s1", "question_id": "Q7", "text": "…", "source_ts_ms": 421000, "gap_type": "PLAN_NO_FALLBACK" }
{ "type": "question_resolved", "session_id": "s1", "question_id": "Q7", "by_chunk": 12 }
{ "type": "question_stale",    "session_id": "s1", "question_id": "Q7" }
{ "type": "pipeline_status",   "session_id": "s1", "status": "idle|processing|degraded" }
{ "type": "final_review",      "session_id": "s1", "top": [ … ], "decisions": [ … ] }
```

Commands from UI to backend:

```
set_goal(session_id, text)
set_meeting_type(session_id, type)
question_feedback(session_id, question_id, asked|dismissed|useful|not_useful)
end_session(session_id)
```

UI requirements:
- The state label comes from `state` only. A card cannot say "OPEN" and "Resolved" at once.
- The timestamp shown is `source_ts_ms` (the time in the call that triggered the question), not 00:00.
- Show a maximum of 2 open questions, plus a collapsed "earlier" list.
- Provide thumbs up / down and "I asked this". These produce the labels for tuning.
- In the Analyse dialog, add a **Goal** text field beside Perspective and Context. The goal is the strongest input to the pipeline. Make "Meeting type" include an Architecture / Technical review option, and warn when the chosen perspective does not match the content (for example Sales applied to an internal review).

---

## 16. Configuration

```toml
[pipeline]
chunk_chars        = 500
chunk_seconds      = 60
hold_seconds       = 75
max_open_questions = 2
min_gap_between_questions_s = 180
stale_after_s      = 600
warmup_chunks      = 2

[models]
understand = "gpt-4o-mini"
write      = "gpt-4o-mini"
judge      = "gpt-4.1"
end_review = "gpt-4.1"

[limits]
llm_timeout_s     = 20
max_open_points   = 15
summary_max_chars = 1200
```

All values are overridable per session for experiments.

---

## 17. Observability

- Structured logs with `tracing` (`session_id`, `chunk_id`, `stage`, `latency_ms`).
- Every stage writes a `pipeline_runs` row, so a bad question can be traced from output back to its inputs.
- A debug view in the UI (hidden behind a dev flag): per chunk, show clean chunk, summary, gaps found, candidate, judge verdict, and why it was dropped.
- Metrics: questions per session, empty-chunk rate, judge rejection reasons, tokens and estimated cost per call hour, p95 latency per stage.

---

## 18. Testing and evaluation

### 18.1 Unit tests
- Chunker: 500-char and 60-s triggers, sentence-boundary wait, flush.
- Guards: banned regex, dedup similarity, attribution check, glossary leak check.
- Memory: bounds and eviction, serialization round-trip, schema migration.
- QuestionStore: state transitions (Pending to Open, Open to Resolved, Open to Stale) and invalid transitions rejected.

### 18.2 Replay harness
A CLI that replays a saved transcript through the pipeline in simulated time.

```
cargo run --bin replay -- --transcript fixtures/glue_fivetran.txt --goal "Stay on Glue or move to Fivetran?"
```

Output: chunk-by-chunk table of gaps, candidates, judge verdicts, and the final surfaced list.

### 18.3 Golden fixtures
Store 8-10 real transcripts, each with a label file:

```yaml
transcript: glue_fivetran.txt
must_surface:         # at least one of each group should appear
  - "annual cost Fivetran hybrid vs Glue"
  - "why not fix Glue / truncate-and-load + CloudWatch"
  - "hybrid skill gap"
must_not_surface:
  - "What does normal five-tran running entail?"
  - "What orchestrator options are you considering instead of Airflow?"
  - "What specific timeframe are you referring to for the switchover?"
max_questions_per_10_min: 2
```

Fixtures from this project:
1. Glue vs Fivetran architecture review (the original 17 low-level questions).
2. Step Functions / dbt trigger snippet.
3. Switchover and purge discussion (the 4 low-level questions).
4. Leadership and storytelling talk (should surface nothing).
5. Sales discovery call (a gap-heavy case).

### 18.4 Regression gate
CI runs the replay harness with a recorded model (cached responses) for deterministic tests, plus a nightly run against the real model. The nightly run fails if any `must_not_surface` pattern appears, or if the question count exceeds the cap.

### 18.5 Concurrency test
Two sessions with different transcripts run in parallel. Assert zero cross-contamination (see 11.4).

---

## 19. Error handling

| Failure | Behaviour |
|---|---|
| LLM timeout / 5xx | Retry once, then skip this chunk. Memory unchanged. Log the run |
| Invalid JSON despite schema | Skip chunk, log raw output, do not retry |
| Judge unavailable | Do not surface the question (fail closed) |
| SQLite write failure | Keep memory in RAM, retry on the next tick, show a warning in dev mode |
| App crash mid-chunk | On restart, reload last saved memory; at most one chunk is lost |
| Summary corrupted or empty | Rebuild from `recent_text` and keep `open_points` |
| Channel full | Drop oldest lines with a warning counter |

Fail closed everywhere. A missing question is cheaper than a wrong one.

---

## 20. Security and privacy

- Transcripts can contain confidential business data (the Fivetran discussion itself was about data leaving the perimeter). State clearly in settings what is sent to the LLM provider.
- Provide a setting to point `base_url` at a private deployment (Azure OpenAI, a local model server) without code changes.
- Store the API key in the OS keychain, not in the config file.
- Allow per-session deletion of transcript, memory, questions, and run logs.
- Do not log full prompts in production by default. Make it a debug toggle.

---

## 21. Suggested module layout

```
src/
  main.rs
  config.rs
  session/
    mod.rs            // SessionRegistry
    actor.rs          // SessionActor, select loop
    memory.rs         // Memory, bounds, migration
    chunker.rs
  pipeline/
    mod.rs            // process_chunk
    understand.rs     // stage 1
    write.rs          // stage 2
    judge.rs          // stage 3
    release.rs        // stage 4 hold + release
    end_review.rs
    guards.rs         // regex, dedup, attribution
  llm/
    client.rs
    schemas.rs        // JSON schemas for structured outputs
    usage.rs
  prompts/
    mod.rs            // PromptStore (template loading, versions)
  storage/
    mod.rs
    migrations/
  events.rs           // UiEvent definitions
  bin/
    replay.rs         // evaluation harness
prompts/              // *.md templates
fixtures/             // transcripts + labels
```

Suggested crates: `tokio`, `reqwest`, `serde`, `serde_json`, `sqlx` (sqlite), `tracing`, `anyhow`/`thiserror`, `indexmap`, `regex`, `minijinja`, `tokio-util`.

---

## 22. Implementation phases

### Phase 0: Foundations (2-3 days)
- Session actor, registry, per-session isolation, SQLite schema.
- Fix the state leak between sessions and the OPEN/Resolved label and 00:00 timestamp bugs.
- Add the replay harness skeleton and load the five fixtures.

**Exit:** two sessions run in parallel with no contamination; replay runs end to end with a stubbed LLM.

### Phase 1: Chunker and Stage 1 (3-4 days)
- Chunker with 500 char / 60 s triggers and speaker labels.
- `LlmClient` with structured outputs, retries, and run logging.
- Stage 1 prompt: clean chunk, summary, gaps, answered ids.
- Glossary support (user-editable, auto-suggested later).

**Exit:** on all fixtures, summaries read like decent meeting notes; most chunks return no gaps.

### Phase 2: Write, judge, guards (3-4 days)
- Stage 2 template-based question writer.
- Stage 3 judge with binary checks.
- Code guards (banned regex, dedup, attribution, single ask).

**Exit:** none of the known bad questions appear in replay; the known good ones appear.

### Phase 3: Hold, release, lifecycle (2-3 days)
- Pending queue, timer release, auto-resolve from later chunks, stale handling.
- UI events and the UI changes in section 15 (goal field, feedback buttons, max 2 open).

**Exit:** a question answered within 75 s of being generated never reaches the UI.

### Phase 4: End-of-meeting pass and feedback loop (2 days)
- End review call, final list in the UI.
- Persist feedback and build a labelled dataset.

**Exit:** the final list for fixture 1 contains the cost, hybrid-skill-gap, and "why not fix Glue" questions.

### Phase 5: Tuning and hardening (ongoing)
- Tune thresholds from real usage data.
- Add embedding-based dedup, summary-quality guard, and a cost dashboard.
- Evaluate whether a stronger model for Stage 1 is worth the cost.

Total: roughly 2.5 to 3.5 weeks for one engineer.

---

## 23. Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Small model does the Stage 1 job poorly (merges memory update and gap detection) | Medium | Split into two calls (update memory, then detect gaps); use a stronger model for Stage 1 if replay shows misses |
| Summary drifts and loses details over a long call | Medium | `recent_text`, must-keep list in the prompt, summary-quality guard |
| Too few questions surface (over-filtering) | Medium | Collapsed "more suggestions" list, tune the guards using the labelled dataset |
| Goal is wrong or missing | High | Ask for it in the Analyse dialog; infer after 3 chunks and let the user edit; use the stricter filter while inferred |
| Mishearing not in the glossary | High | Glossary auto-suggestions from the user, 'term correction' button in the transcript view |
| Latency makes questions arrive late | Low | 75 s hold is intentional; shorten it per meeting type if testing shows questions go stale |
| LLM cost grows on long calls | Low | Stages 2 and 3 run on at most one candidate per chunk; summary and open points are bounded |
| Confidential data sent to a third party | Medium | Private endpoint option, clear settings text, per-session deletion |

---

## 24. Open questions for the team

1. Is the transcript source able to provide speaker labels (presenter vs others)? Attribution quality depends on it.
2. Will the goal be required, optional, or inferred by default?
3. Which provider endpoints are acceptable for confidential calls?
4. Do users want the end-of-meeting list exported (to notes, email, or a ticket)?
5. What is the target cost per hour of call?

---

## 25. Definition of done

- Replay on all fixtures passes: no `must_not_surface` question appears, and at least one question per `must_surface` group does.
- Average at most 2 open questions per 10 minutes on real calls.
- Zero cross-session contamination in tests.
- Every surfaced question is traceable in logs from output to inputs.
- Users can set a goal, give feedback on each question, and see a final unresolved-items list at call end.