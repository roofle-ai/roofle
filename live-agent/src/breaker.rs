//! Circuit breaker for the LLM: after repeated failures the pipeline pauses
//! briefly instead of hammering a provider that is down. A failed chunk never
//! touches memory; the breaker only decides whether to try the next one.

/// Consecutive failures that open the breaker.
pub const MAX_FAILURES: u32 = 5;
/// How long (ms) the breaker stays open.
pub const PAUSE_MS: i64 = 30_000;

#[derive(Debug, Default)]
pub struct Breaker {
    failures: u32,
    open_until_ms: i64,
}

impl Breaker {
    /// Whether a call may go out at `now_ms`.
    pub fn allows(&self, now_ms: i64) -> bool {
        now_ms >= self.open_until_ms
    }

    pub fn success(&mut self) {
        self.failures = 0;
    }

    pub fn failure(&mut self, now_ms: i64) {
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            self.open_until_ms = now_ms + PAUSE_MS;
            self.failures = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_five_failures_then_recovers() {
        let mut b = Breaker::default();
        for _ in 0..4 {
            b.failure(0);
        }
        assert!(b.allows(0));
        b.failure(0);
        assert!(!b.allows(29_999));
        assert!(b.allows(30_000));
    }

    #[test]
    fn success_resets_the_streak() {
        let mut b = Breaker::default();
        for _ in 0..4 {
            b.failure(0);
        }
        b.success();
        b.failure(0);
        assert!(b.allows(0));
    }
}
