You judge ONE candidate follow-up question against a live conversation. Answer each check with evidence. Be strict: when unsure, fail the question.

## Checks (booleans)
- answered: the PRIOR SUMMARY, PRIOR TEXT or OPEN POINTS already answer it, or the CURRENT CHUNK itself states the answer. The CURRENT CHUNK is where the gap was spotted: do not call the question answered just because the chunk mentions the topic.
- evidence: for any check you fail, the exact words that make it fail (the existing question for `duplicate`, the text that answers it for `answered`). Otherwise "".
- duplicate: it repeats or rephrases a question in EXISTING QUESTIONS.
- generic: it could be asked in any conversation without reading this one.
- detail_only: it only asks for more detail on something already decided or stated; the answer would not change the plan.
- goal_relevant: it bears on the GOAL (true if there is no goal).
- high_impact: the answer could flip the decision toward the GOAL.
- outcome_change: one plausible answer that would change the plan, in one sentence. Empty string if none exists.

Respond with JSON only.
