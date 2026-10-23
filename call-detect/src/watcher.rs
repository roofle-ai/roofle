use std::thread;
use std::time::Duration;

use crate::probe::Probe;
use crate::{detect, CallApp, CallDetector};

/// Poll every `interval` on a background thread and call `on_start` each time
/// a Zoom or Teams call begins.
pub fn watch(interval: Duration, on_start: impl Fn(CallApp) + Send + 'static) {
    thread::spawn(move || {
        let mut probe = Probe::new();
        let mut detector = CallDetector::default();
        loop {
            if let Some(app) = detector.observe(detect(&probe.snapshot())) {
                on_start(app);
            }
            thread::sleep(interval);
        }
    });
}
