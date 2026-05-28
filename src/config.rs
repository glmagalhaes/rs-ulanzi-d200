use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;


pub const WINDOW_MODE_STATUS: u8 = 0;
pub const WINDOW_MODE_CLOCK: u8 = 1;
pub const WINDOW_MODE_CLEAR: u8 = 2;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    #[serde(default)]
    pub label_style: HashMap<String, serde_json::Value>,
    #[serde(default = "default_display_mode")]
    pub display_mode: u8,
    #[serde(default = "default_stats_interval")]
    pub stats_interval_ms: u64,
    #[serde(skip)]
    pub filepath: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            brightness: default_brightness(),
            label_style: HashMap::new(),
            display_mode: default_display_mode(),
            stats_interval_ms: default_stats_interval(),
            filepath: None,
        }
    }
}

fn default_brightness() -> u8 {
    100
}
fn default_display_mode() -> u8 {
    return WINDOW_MODE_STATUS as u8;
}
fn default_stats_interval() -> u64 {
    1000
}



impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let mut config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config: {:?}", path))?;

        config.filepath = Some(path.to_path_buf());

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(ref path) = self.filepath {
            let content = serde_yaml::to_string(self)
                .with_context(|| "Failed to serialize config")?;
            fs::write(path, content)
                .with_context(|| format!("Failed to write config file: {:?}", path))?;
        }
        Ok(())
    }
}
