use std::time::{Duration, Instant};

/// Recording time excluding paused stretches, so timestamps track the spoken
/// audio across pause/resume.
#[derive(Default)]
pub struct RecordingClock {
    started: Option<Instant>,
    paused_total: Duration,
    pause_started: Option<Instant>,
}

impl RecordingClock {
    pub fn start(&mut self) {
        *self = Self {
            started: Some(Instant::now()),
            ..Self::default()
        };
    }

    pub fn pause(&mut self) {
        self.pause_started.get_or_insert_with(Instant::now);
    }

    pub fn resume(&mut self) {
        if let Some(since) = self.pause_started.take() {
            self.paused_total += since.elapsed();
        }
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.started
            .map(|s| s.elapsed().saturating_sub(self.paused_total).as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Elapsed seconds, then reset to idle.
    pub fn finish(&mut self) -> f64 {
        let secs = self.elapsed_secs();
        *self = Self::default();
        secs
    }
}
