# Replay fixtures

Each fixture is `<name>.txt` (lines like `MM:SS Presenter|Other: text`), optional
`<name>.labels.json` (`goal`, `must_surface`, `must_not_surface`,
`max_questions_per_10_min`) and a recorded `<name>.cache.json`.

Record or refresh a cache against a real model (run from the repo root):

    OPENROUTER_API_KEY=... ROOFLE_MODEL=openai/gpt-4o-mini \
      cargo run -p live-agent --example replay -- live-agent/fixtures/<name>.txt

`cargo test -p live-agent` replays every fixture that has a cache and fails on
any label violation. Any prompt or schema change invalidates the cache (a
miss is an error): delete `<name>.cache.json` and re-record.
