use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

pub struct SystemMonitor {
    sys: System,
}

impl SystemMonitor {
    pub fn new() -> Self {
        // Initialize with partial refresh to save resources
        let mut sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();
        Self { sys }
    }

    pub fn get_metrics(&mut self) -> (u8, u8) {
        // Refresh CPU and Memory
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();

        let cpu_usage = self.sys.global_cpu_info().cpu_usage() as u8;

        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let mem_usage = if total_mem > 0 {
            ((used_mem as f64 / total_mem as f64) * 100.0) as u8
        } else {
            0
        };

        (cpu_usage, mem_usage)
    }
}
