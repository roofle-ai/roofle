use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::{mic, Snapshot};

/// Reads the machine's current state, reusing one process table between polls.
pub struct Probe {
    system: System,
}

impl Probe {
    pub fn new() -> Self {
        Self { system: System::new() }
    }

    pub fn snapshot(&mut self) -> Snapshot {
        self.system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        Snapshot {
            processes: self
                .system
                .processes()
                .values()
                .map(|p| p.name().to_string_lossy().to_lowercase())
                .collect(),
            mic_active: mic::is_active(),
        }
    }
}
