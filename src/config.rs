use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[repr(u8)]
pub enum WindowMode{
    Status = 0,
    Clock = 1,
    Clear = 2, 
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ButtonConfig {
    #[serde(skip)]
    pub index: usize,
    pub image: Option<String>,
    pub label: String,
    #[serde(rename = "action")]
    pub action_type: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub state: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    #[serde(default)]
    pub label_style: HashMap<String, serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_buttons")]
    pub buttons: Vec<ButtonConfig>,
    #[serde(default = "default_display_mode")]
    pub display_mode: u8,
    #[serde(default = "default_stats_interval")]
    pub stats_interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            brightness: default_brightness(),
            label_style: HashMap::new(),
            buttons: Vec::new(),
            display_mode: default_display_mode(),
            stats_interval_ms: default_stats_interval(),
        }
    }
}

fn default_brightness() -> u8 {
    100
}
fn default_display_mode() -> u8 {
    return WindowMode::Clock as u8;
} // Default to CLOCK
fn default_stats_interval() -> u64 {
    1000
}

fn deserialize_buttons<'de, D>(deserializer: D) -> Result<Vec<ButtonConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_buttons: Vec<Option<ButtonConfig>> = Deserialize::deserialize(deserializer)?;
    let mut buttons = Vec::new();
    for (index, raw_btn) in raw_buttons.into_iter().enumerate() {
        if let Some(mut btn) = raw_btn {
            btn.index = index;
            buttons.push(btn);
        }
    }
    Ok(buttons)
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let mut config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config: {:?}", path))?;

        // Resolve image paths relative to config file
        if let Some(parent) = path.parent() {
            for button in &mut config.buttons {
                if let Some(ref img_path) = button.image {
                    let p = Path::new(img_path);
                    if !p.is_absolute() {
                        let absolute = parent.join(p);
                        button.image = Some(absolute.to_string_lossy().into_owned());
                    }
                }
            }
        }

        Ok(config)
    }
}
