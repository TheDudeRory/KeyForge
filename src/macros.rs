use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

/// A step/condition parameter: either a literal or `{ "expr": "..." }`,
/// evaluated with Rhai against the macro's variables at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Param<T> {
    Expr { expr: String },
    Literal(T),
}

impl<T> From<T> for Param<T> {
    fn from(v: T) -> Self {
        Param::Literal(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Contains,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    All { conditions: Vec<Condition> },
    Any { conditions: Vec<Condition> },
    Not { condition: Box<Condition> },
    /// Escape hatch: arbitrary Rhai boolean expression.
    Expr { expr: String },
    VariableComparison { variable: String, op: CmpOp, value: serde_json::Value },
    // OS-backed conditions (window/process/device/pixel/...) land with M5/M6.
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

fn default_poll_ms() -> u64 {
    100
}
fn default_wait_timeout_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    If {
        condition: Condition,
        #[serde(default)]
        then: Vec<Step>,
        #[serde(default, rename = "else")]
        else_steps: Vec<Step>,
    },
    Loop {
        times: Param<i64>,
        #[serde(default)]
        steps: Vec<Step>,
    },
    While {
        condition: Condition,
        #[serde(default)]
        steps: Vec<Step>,
    },
    Break,
    Wait {
        ms: Param<i64>,
    },
    WaitUntil {
        condition: Condition,
        #[serde(default = "default_poll_ms")]
        poll_ms: u64,
        #[serde(default = "default_wait_timeout_ms")]
        timeout_ms: u64,
        #[serde(default)]
        on_timeout: Vec<Step>,
    },
    SetVariable {
        name: String,
        value: Param<serde_json::Value>,
    },
    StopMacro,
    RunMacro {
        id: String,
    },
    LaunchProgram {
        path: Param<String>,
        #[serde(default)]
        args: Vec<Param<String>>,
    },
    SendKeystroke {
        keys: Param<String>,
    },
    TypeText {
        text: Param<String>,
        #[serde(default)]
        char_delay_ms: u64,
    },
    HoldKey {
        key: Param<String>,
    },
    ReleaseKey {
        key: Param<String>,
    },
    MouseMove {
        x: Param<i64>,
        y: Param<i64>,
        // ponytail: absolute/relative only; window-relative mode arrives with
        // the WindowManager in M5.
        #[serde(default)]
        relative: bool,
    },
    MouseClick {
        #[serde(default)]
        button: MouseButton,
        #[serde(default)]
        double: bool,
    },
    MouseDrag {
        from_x: Param<i64>,
        from_y: Param<i64>,
        to_x: Param<i64>,
        to_y: Param<i64>,
        #[serde(default)]
        button: MouseButton,
    },
    Scroll {
        direction: ScrollDirection,
        amount: Param<i64>,
    },
}

fn param_summary<T: std::fmt::Display>(p: &Param<T>) -> String {
    match p {
        Param::Literal(v) => v.to_string(),
        Param::Expr { expr } => format!("({expr})"),
    }
}

impl Step {
    /// One-line human-readable summary (log lines now, collapsed blocks in M7).
    pub fn summary(&self) -> String {
        match self {
            Step::If { .. } => "If".into(),
            Step::Loop { times, .. } => format!("Loop {} times", param_summary(times)),
            Step::While { .. } => "While".into(),
            Step::Break => "Break".into(),
            Step::Wait { ms } => format!("Wait {} ms", param_summary(ms)),
            Step::WaitUntil { timeout_ms, .. } => format!("Wait until (timeout {timeout_ms} ms)"),
            Step::SetVariable { name, value } => match value {
                Param::Expr { expr } => format!("Set {name} = ({expr})"),
                Param::Literal(v) => format!("Set {name} = {v}"),
            },
            Step::StopMacro => "Stop macro".into(),
            Step::RunMacro { id } => format!("Run macro {id}"),
            Step::LaunchProgram { path, .. } => format!("Launch {}", param_summary(path)),
            Step::SendKeystroke { keys } => format!("Press {}", param_summary(keys)),
            Step::TypeText { text, .. } => {
                let t = param_summary(text);
                let short: String = t.chars().take(24).collect();
                format!("Type {short:?}{}", if t.chars().count() > 24 { "…" } else { "" })
            }
            Step::HoldKey { key } => format!("Hold {}", param_summary(key)),
            Step::ReleaseKey { key } => format!("Release {}", param_summary(key)),
            Step::MouseMove { x, y, relative } => {
                let how = if *relative { "by" } else { "to" };
                format!("Mouse move {how} ({}, {})", param_summary(x), param_summary(y))
            }
            Step::MouseClick { button, double } => {
                format!("{} {button:?}", if *double { "Double-click" } else { "Click" })
            }
            Step::MouseDrag { from_x, from_y, to_x, to_y, button } => format!(
                "Drag {button:?} ({}, {}) → ({}, {})",
                param_summary(from_x),
                param_summary(from_y),
                param_summary(to_x),
                param_summary(to_y)
            ),
            Step::Scroll { direction, amount } => {
                format!("Scroll {direction:?} {}", param_summary(amount))
            }
        }
    }
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Runaway-guard overrides; defaults in exec.rs apply when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runtime_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_loop_iterations: Option<u64>,
}

/// All macros from `keyforge_data/macros/`, one JSON file each.
/// ponytail: re-read from disk on every trigger fire — files are tiny and
/// hand-edited in M3; add caching/file-watch only if profiling ever cares.
#[derive(Debug, Default, Clone)]
pub struct MacroLibrary {
    macros: HashMap<String, Macro>,
}

impl MacroLibrary {
    pub fn load(dir: &Path) -> MacroLibrary {
        let mut macros = HashMap::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                tracing::error!(dir = %dir.display(), error = %e, "cannot read macros directory");
                return MacroLibrary::default();
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| serde_json::from_str::<Macro>(&text).map_err(|e| e.to_string()));
            match parsed {
                // A broken macro file is skipped, never replaced with defaults.
                Err(e) => tracing::error!(file = %path.display(), error = %e, "skipping unreadable macro"),
                Ok(m) => {
                    if let Some(old) = macros.insert(m.id.clone(), m) {
                        tracing::warn!(id = %old.id, "duplicate macro id; keeping the later file");
                    }
                }
            }
        }
        MacroLibrary { macros }
    }

    pub fn get(&self, id: &str) -> Option<&Macro> {
        self.macros.get(id)
    }

    /// Sorted by name for stable UI listing.
    pub fn iter_sorted(&self) -> Vec<&Macro> {
        let mut v: Vec<&Macro> = self.macros.values().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

/// First-run template so hand-writing macros starts from a working file.
pub fn seed_example(dir: &Path) {
    let has_any = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json")))
        .unwrap_or(false);
    if has_any {
        return;
    }
    let example = Macro {
        schema_version: SCHEMA_VERSION,
        id: "example".into(),
        name: "Example: count to three".into(),
        description: "Hand-written example. Copy this file to create your own macro; \
                      the visual editor arrives in a later milestone."
            .into(),
        steps: vec![
            Step::SetVariable { name: "n".into(), value: serde_json::json!(0).into() },
            Step::Loop {
                times: 3.into(),
                steps: vec![
                    Step::SetVariable { name: "n".into(), value: Param::Expr { expr: "n + 1".into() } },
                    Step::Wait { ms: 250.into() },
                ],
            },
            Step::If {
                condition: Condition::VariableComparison {
                    variable: "n".into(),
                    op: CmpOp::Eq,
                    value: serde_json::json!(3),
                },
                then: vec![Step::SetVariable {
                    name: "result".into(),
                    value: serde_json::json!("counted to three!").into(),
                }],
                else_steps: vec![Step::StopMacro],
            },
        ],
        max_runtime_ms: None,
        max_loop_iterations: None,
    };
    crate::persist::save(&dir.join("example.json"), &example);
    tracing::info!("seeded macros/example.json");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_untagged_forms() {
        let lit: Param<i64> = serde_json::from_str("5").unwrap();
        assert_eq!(lit, Param::Literal(5));
        let expr: Param<i64> = serde_json::from_str(r#"{"expr": "n + 1"}"#).unwrap();
        assert_eq!(expr, Param::Expr { expr: "n + 1".into() });
        // a literal JSON value that isn't an expr object stays literal
        let val: Param<serde_json::Value> = serde_json::from_str(r#"{"a": 1}"#).unwrap();
        assert!(matches!(val, Param::Literal(_)));
    }

    #[test]
    fn macro_roundtrip() {
        let m = Macro {
            schema_version: SCHEMA_VERSION,
            id: "t".into(),
            name: "t".into(),
            description: String::new(),
            steps: vec![
                Step::If {
                    condition: Condition::All {
                        conditions: vec![Condition::Expr { expr: "true".into() }],
                    },
                    then: vec![Step::Break],
                    else_steps: vec![],
                },
                Step::WaitUntil {
                    condition: Condition::VariableComparison {
                        variable: "x".into(),
                        op: CmpOp::Gt,
                        value: serde_json::json!(2),
                    },
                    poll_ms: 50,
                    timeout_ms: 1000,
                    on_timeout: vec![Step::StopMacro],
                },
            ],
            max_runtime_ms: Some(1000),
            max_loop_iterations: None,
        };
        let json = serde_json::to_string_pretty(&m).unwrap();
        assert_eq!(serde_json::from_str::<Macro>(&json).unwrap(), m);
        assert!(json.contains("\"type\": \"wait_until\""));
        assert!(json.contains("\"else\""));
    }

    #[test]
    fn missing_optional_fields_default() {
        let m: Macro = serde_json::from_str(r#"{"id": "x", "name": "X"}"#).unwrap();
        assert_eq!(m.schema_version, SCHEMA_VERSION);
        assert!(m.steps.is_empty());
        let s: Step = serde_json::from_str(r#"{"type": "wait_until", "condition": {"type": "expr", "expr": "true"}}"#).unwrap();
        assert!(matches!(s, Step::WaitUntil { poll_ms: 100, timeout_ms: 10_000, .. }));
    }
}
