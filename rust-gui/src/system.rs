use sysinfo::System;

#[derive(Debug)]
pub struct SystemMonitor {
    system: System,
}

#[derive(Debug, Clone, Default)]
pub struct SystemSnapshot {
    pub cpu_percent: f32,
    pub memory_used_gib: f64,
    pub memory_total_gib: f64,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        Self { system }
    }

    pub fn snapshot(&mut self) -> SystemSnapshot {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let gib = 1024.0 * 1024.0 * 1024.0;
        SystemSnapshot {
            cpu_percent: self.system.global_cpu_usage(),
            memory_used_gib: self.system.used_memory() as f64 / gib,
            memory_total_gib: self.system.total_memory() as f64 / gib,
        }
    }
}
