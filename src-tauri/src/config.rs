//! Non-sensitive user preferences, persisted as TOML at
//! `~/.config/agent-plan-monitor/config.toml`.

use serde::{Deserialize, Serialize};

pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 300; // 5 minutes
pub const ACCELERATED_POLL_INTERVAL_SECS: u64 = 60; // when a window >= 80%
pub const ACCELERATION_THRESHOLD: f64 = 0.8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Base polling interval in seconds.
    pub poll_interval_secs: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join(".config/agent-plan-monitor/config.toml"))
}

pub fn load() -> AppConfig {
    config_path()
        .map(|p| load_from(&p))
        .unwrap_or_default()
}

pub fn load_from(path: &std::path::Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path().ok_or("no home dir")?;
    save_to(&path, cfg)
}

pub fn save_to(path: &std::path::Path, cfg: &AppConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let content = toml::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let c = AppConfig::default();
        assert_eq!(c.poll_interval_secs, 300);
    }

    #[test]
    fn toml_roundtrip() {
        let c = AppConfig {
            poll_interval_secs: 120,
        };
        let s = toml::to_string(&c).unwrap();
        let back: AppConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.poll_interval_secs, 120);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let back: AppConfig = toml::from_str("poll_interval_secs = 60").unwrap();
        assert_eq!(back.poll_interval_secs, 60);
    }

    #[test]
    fn old_config_with_show_toggles_still_loads() {
        // Configs written before the show_* toggles were removed must not
        // break deserialization (serde ignores unknown fields).
        let back: AppConfig =
            toml::from_str("poll_interval_secs = 60\nshow_kimi = false\nshow_ark = false\nshow_kiro = false\n")
                .unwrap();
        assert_eq!(back.poll_interval_secs, 60);
    }

    #[test]
    fn file_persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("apm-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        let c = AppConfig {
            poll_interval_secs: 90,
        };
        save_to(&path, &c).unwrap();
        let back = load_from(&path);
        assert_eq!(back.poll_interval_secs, 90);
        // No secrets ever land in the config file.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.to_lowercase().contains("secret"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let back = load_from(std::path::Path::new("/nonexistent/config.toml"));
        assert_eq!(back.poll_interval_secs, DEFAULT_POLL_INTERVAL_SECS);
    }
}
