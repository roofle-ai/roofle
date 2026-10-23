use crate::CallApp;

/// Reports a call once when it starts, not on every poll while it lasts.
#[derive(Debug, Default)]
pub struct CallDetector {
    current: Option<CallApp>,
}

impl CallDetector {
    /// Feed the latest [`detect`](crate::detect) result; `Some` only when a call just started.
    pub fn observe(&mut self, now: Option<CallApp>) -> Option<CallApp> {
        let started = now.filter(|app| self.current != Some(*app));
        self.current = now;
        started
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_call_once() {
        let mut d = CallDetector::default();
        assert_eq!(d.observe(None), None);
        assert_eq!(d.observe(Some(CallApp::Zoom)), Some(CallApp::Zoom));
        assert_eq!(d.observe(Some(CallApp::Zoom)), None);
    }

    #[test]
    fn reports_again_after_the_call_ends() {
        let mut d = CallDetector::default();
        d.observe(Some(CallApp::Zoom));
        d.observe(None);
        assert_eq!(d.observe(Some(CallApp::Zoom)), Some(CallApp::Zoom));
    }

    #[test]
    fn reports_a_switch_to_another_app() {
        let mut d = CallDetector::default();
        d.observe(Some(CallApp::Teams));
        assert_eq!(d.observe(Some(CallApp::Zoom)), Some(CallApp::Zoom));
    }
}
