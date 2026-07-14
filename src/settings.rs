use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub active_profile: String,
    pub start_minimized: bool,
    pub minimize_to_tray_on_close: bool,
    pub theme: Theme,
    pub emergency_stop_hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: SCHEMA_VERSION,
            active_profile: "default".into(),
            start_minimized: false,
            minimize_to_tray_on_close: true,
            theme: Theme::Dark,
            emergency_stop_hotkey: "Ctrl+Alt+End".into(),
        }
    }
}

// ponytail: identity migration; add per-version steps here when SCHEMA_VERSION bumps.
fn migrate(mut s: Settings) -> Settings {
    if s.schema_version < SCHEMA_VERSION {
        tracing::info!(from = s.schema_version, to = SCHEMA_VERSION, "migrating settings");
        s.schema_version = SCHEMA_VERSION;
    }
    s
}

impl Settings {
    /// Loads settings, creating the file with defaults if missing. A corrupt
    /// file is preserved as `settings.json.bak` instead of being overwritten.
    pub fn load_or_create(path: &Path) -> Settings {
        let settings = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => migrate(s),
                Err(e) => {
                    let bak = path.with_extension("json.bak");
                    tracing::error!(error = %e, backup = %bak.display(), "settings.json is corrupt, using defaults");
                    let _ = std::fs::rename(path, &bak);
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        settings.save(path);
        settings
    }

    pub fn save(&self, path: &Path) {
        // Struct field order gives stable key ordering; pretty-printed for clean diffs.
        let json = serde_json::to_string_pretty(self).expect("settings always serialize");
        if let Err(e) = std::fs::write(path, json) {
            tracing::error!(error = %e, path = %path.display(), "failed to save settings");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let s = Settings::default();
        let json = serde_json::to_string_pretty(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        // unknown/missing fields must not break old files
        let sparse: Settings = serde_json::from_str(r#"{"schema_version": 0, "future_field": 1}"#).unwrap();
        assert_eq!(migrate(sparse).schema_version, SCHEMA_VERSION);
    }
}
