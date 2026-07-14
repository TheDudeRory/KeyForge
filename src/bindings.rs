use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    // ponytail: inline action until M3; bindings will then reference macro ids
    // and Launch Program becomes a regular macro step.
    LaunchProgram {
        path: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl Action {
    pub fn summary(&self) -> String {
        match self {
            Action::LaunchProgram { path, args } if args.is_empty() => format!("Launch {path}"),
            Action::LaunchProgram { path, args } => format!("Launch {path} {}", args.join(" ")),
        }
    }

    pub fn execute(&self) {
        match self {
            Action::LaunchProgram { path, args } => {
                match std::process::Command::new(path).args(args).spawn() {
                    Ok(child) => tracing::info!(path, pid = child.id(), "launched program"),
                    Err(e) => tracing::error!(path, error = %e, "failed to launch program"),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub hotkey: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub action: Action,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub schema_version: u32,
    pub bindings: Vec<Binding>,
}

impl Default for Profile {
    fn default() -> Self {
        // Seed with one working example so a fresh install demonstrates the loop.
        #[cfg(windows)]
        let action = Action::LaunchProgram { path: "notepad.exe".into(), args: vec![] };
        #[cfg(not(windows))]
        let action = Action::LaunchProgram { path: "xdg-open".into(), args: vec![".".into()] };
        Profile {
            schema_version: SCHEMA_VERSION,
            bindings: vec![Binding { hotkey: "Ctrl+Alt+K".into(), enabled: true, action }],
        }
    }
}

// ponytail: identity migration; add per-version steps when SCHEMA_VERSION bumps.
fn migrate(mut p: Profile) -> Profile {
    if p.schema_version < SCHEMA_VERSION {
        tracing::info!(from = p.schema_version, to = SCHEMA_VERSION, "migrating profile");
        p.schema_version = SCHEMA_VERSION;
    }
    p
}

impl Profile {
    pub fn load_or_create(path: &Path) -> Profile {
        crate::persist::load_or_create(path, migrate)
    }

    pub fn save(&self, path: &Path) {
        crate::persist::save(path, self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = Profile::default();
        let json = serde_json::to_string_pretty(&p).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);
        assert!(json.contains("\"type\": \"launch_program\""));
    }
}
