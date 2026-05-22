use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use std::fs;
use std::path::Path;

#[cfg(feature = "nvidia")]
use nvml_wrapper::Nvml;

pub struct SystemMonitor {
    sys: System,
    #[cfg(feature = "nvidia")]
    nvml: Option<Nvml>,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();

        #[cfg(feature = "nvidia")]
        let nvml = Nvml::init().ok();

        Self {
            sys,
            #[cfg(feature = "nvidia")]
            nvml,
        }
    }

    /// Returns (CPU usage %, Memory usage %, GPU usage %)
    pub fn get_metrics(&mut self) -> (u8, u8, u8) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();

        let cpu_usage = self.sys.global_cpu_usage() as u8;

        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let mem_usage = if total_mem > 0 {
            ((used_mem as f64 / total_mem as f64) * 100.0) as u8
        } else {
            0
        };

        let gpu_usage = self.get_gpu_load();

        (cpu_usage, mem_usage, gpu_usage)
    }

    /// Returns the utilisation percentage of the first usable GPU.
    /// Supports NVIDIA (via NVML) and AMD/Intel (via sysfs `gpu_busy_percent`).
    fn get_gpu_load(&self) -> u8 {
        // 1) Try NVIDIA GPUs through NVML
        #[cfg(feature = "nvidia")]
        if let Some(nv) = &self.nvml {
            if let Ok(device_count) = nv.device_count() {
                for i in 0..device_count {
                    if let Ok(device) = nv.device_by_index(i) {
                        if let Ok(util) = device.utilization_rates() {
                            return util.gpu as u8;
                        }
                    }
                }
            }
        }

        // 2) Fallback: scan /sys/class/drm for AMD/Intel cards
        let drm_path = Path::new("/sys/class/drm");
        if let Ok(entries) = fs::read_dir(drm_path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Only consider top‑level `cardN` entries
                if name.starts_with("card") && name.chars().skip(4).all(|c| c.is_ascii_digit()) {
                    // Detect vendor via PCI vendor ID
                    let vendor_path = format!("/sys/class/drm/{}/device/vendor", name);
                    if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                        let vendor = vendor.trim();
                        // AMD or Intel → try gpu_busy_percent
                        if vendor == "0x1002" || vendor == "0x8086" {
                            let busy_path = format!("/sys/class/drm/{}/device/gpu_busy_percent", name);
                            if let Ok(content) = fs::read_to_string(&busy_path) {
                                if let Ok(load) = content.trim().parse::<f32>() {
                                    return load as u8;
                                }
                            }
                        }
                    }
                }
            }
        }

        0
    }
}