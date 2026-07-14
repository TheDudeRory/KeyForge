use crate::bindings::{Action, Profile};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Canonical form used as the map key everywhere: lowercase, no whitespace.
pub fn normalize(hotkey: &str) -> String {
    hotkey
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// OS hotkey registration, replace-all semantics. Strings are normalized combo
/// strings; each one that fails to parse or register comes back with an error.
pub trait HotkeyBackend {
    fn apply(&mut self, hotkeys: &[String]) -> Vec<(String, String)>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum FireTarget {
    EmergencyStop,
    Run(Action),
}

/// normalized hotkey → what to do when it fires. Shared with the OS event
/// callback, which runs off the UI frame loop.
pub type TargetMap = Arc<Mutex<HashMap<String, FireTarget>>>;

pub fn fire(targets: &TargetMap, hotkey: &str, dispatcher: &crate::exec::Dispatcher) {
    let target = targets.lock().unwrap().get(hotkey).cloned();
    match target {
        Some(FireTarget::EmergencyStop) => dispatcher.emergency_stop(),
        Some(FireTarget::Run(action)) => {
            tracing::info!(hotkey, action = %action.summary(), "hotkey fired");
            dispatcher.dispatch(&action);
        }
        None => tracing::warn!(hotkey, "hotkey fired with no bound target"),
    }
}

/// The emergency stop must fire even while a macro holds extra modifiers —
/// RegisterHotKey/XGrabKey match the exact modifier state, so a held Shift
/// would mask the stop combo precisely when it's needed most. Register every
/// modifier superset of the configured combo instead.
fn emergency_supersets(normalized: &str) -> Vec<String> {
    let mut tokens: Vec<&str> = normalized.split('+').collect();
    let key = match tokens.pop() {
        Some(k) if !k.is_empty() => k,
        _ => return vec![normalized.to_string()],
    };
    fn canonical(t: &str) -> &str {
        match t {
            "control" => "ctrl",
            "win" | "cmd" | "command" | "meta" => "super",
            other => other,
        }
    }
    let have: Vec<&str> = tokens.iter().map(|t| canonical(t)).collect();
    const ALL: [&str; 4] = ["ctrl", "alt", "shift", "super"];
    let free: Vec<&str> = ALL.into_iter().filter(|m| !have.contains(m)).collect();
    (0..1u32 << free.len())
        .map(|mask| {
            let mut parts: Vec<&str> = ALL
                .into_iter()
                .filter(|m| {
                    have.contains(m)
                        || free.iter().position(|f| f == m).is_some_and(|i| mask & (1 << i) != 0)
                })
                .collect();
            parts.push(key);
            parts.join("+")
        })
        .collect()
}

pub struct HotkeyEngine {
    backend: Box<dyn HotkeyBackend>,
    targets: TargetMap,
    /// normalized hotkey → why it is not active (conflict / parse / registration error)
    errors: HashMap<String, String>,
}

impl HotkeyEngine {
    pub fn new(backend: Box<dyn HotkeyBackend>, targets: TargetMap) -> Self {
        HotkeyEngine { backend, targets, errors: HashMap::new() }
    }

    /// Recompute the full desired set (emergency stop first, then enabled
    /// bindings) and push it to the OS. Conflicts and failures land in errors.
    pub fn sync(&mut self, emergency_stop: &str, profile: &Profile) {
        self.errors.clear();
        let mut desired: Vec<(String, FireTarget)> = Vec::new();
        let es = normalize(emergency_stop);
        if !es.is_empty() {
            for superset in emergency_supersets(&es) {
                desired.push((superset, FireTarget::EmergencyStop));
            }
        }
        for binding in profile.bindings.iter().filter(|b| b.enabled) {
            let key = normalize(&binding.hotkey);
            if key.is_empty() {
                continue;
            }
            if desired.iter().any(|(k, _)| *k == key) {
                // Badge the combo but keep the first registrant active — a
                // conflicting row must never knock out the emergency stop.
                self.errors.insert(key, "conflict: combo already bound (first binding wins)".into());
                continue;
            }
            desired.push((key, FireTarget::Run(binding.action.clone())));
        }

        let combos: Vec<String> = desired.iter().map(|(k, _)| k.clone()).collect();
        let mut failed = std::collections::HashSet::new();
        for (hotkey, error) in self.backend.apply(&combos) {
            tracing::error!(hotkey, error, "hotkey not registered");
            failed.insert(hotkey.clone());
            self.errors.insert(hotkey, error);
        }

        let mut map = self.targets.lock().unwrap();
        map.clear();
        map.extend(desired.into_iter().filter(|(k, _)| !failed.contains(k)));
        tracing::info!(active = map.len(), errors = self.errors.len(), "hotkeys synced");
    }

    pub fn error_for(&self, hotkey: &str) -> Option<&String> {
        self.errors.get(&normalize(hotkey))
    }
}

/// Real backend over the global-hotkey crate. Must be constructed on the main
/// thread: on Windows registrations bind to a thread with a message pump, and
/// we ride winit's, which keeps pumping even while the window is hidden — so
/// firing never depends on egui repainting.
/// ponytail: move to a dedicated pump thread only if winit's loop ever proves
/// blocking (e.g. modal drags starving WM_HOTKEY).
pub struct GlobalHotkeyBackend {
    manager: global_hotkey::GlobalHotKeyManager,
    registered: Vec<global_hotkey::hotkey::HotKey>,
    ids: Arc<Mutex<HashMap<u32, String>>>,
}

impl GlobalHotkeyBackend {
    pub fn new(targets: TargetMap, dispatcher: crate::exec::Dispatcher) -> Result<Self, String> {
        let manager = global_hotkey::GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
        let ids: Arc<Mutex<HashMap<u32, String>>> = Arc::default();
        let handler_ids = Arc::clone(&ids);
        global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(
            move |event: global_hotkey::GlobalHotKeyEvent| {
                if event.state == global_hotkey::HotKeyState::Pressed {
                    let hotkey = handler_ids.lock().unwrap().get(&event.id).cloned();
                    if let Some(hotkey) = hotkey {
                        fire(&targets, &hotkey, &dispatcher);
                    }
                }
            },
        ));
        Ok(GlobalHotkeyBackend { manager, registered: Vec::new(), ids })
    }
}

impl HotkeyBackend for GlobalHotkeyBackend {
    fn apply(&mut self, hotkeys: &[String]) -> Vec<(String, String)> {
        use std::str::FromStr;
        if let Err(e) = self.manager.unregister_all(&self.registered) {
            tracing::error!(error = %e, "failed to unregister previous hotkeys");
        }
        self.registered.clear();

        let mut errors = Vec::new();
        let mut ids = HashMap::new();
        for combo in hotkeys {
            let result = global_hotkey::hotkey::HotKey::from_str(combo)
                .map_err(|e| e.to_string())
                .and_then(|hk| self.manager.register(hk).map(|()| hk).map_err(|e| e.to_string()));
            match result {
                Ok(hk) => {
                    ids.insert(hk.id(), combo.clone());
                    self.registered.push(hk);
                }
                Err(error) => errors.push((combo.clone(), error)),
            }
        }
        *self.ids.lock().unwrap() = ids;
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::Binding;

    #[derive(Default)]
    struct FakeBackend {
        applied: Arc<Mutex<Vec<Vec<String>>>>,
        fail: HashMap<String, String>,
    }

    impl HotkeyBackend for FakeBackend {
        fn apply(&mut self, hotkeys: &[String]) -> Vec<(String, String)> {
            self.applied.lock().unwrap().push(hotkeys.to_vec());
            hotkeys
                .iter()
                .filter_map(|h| self.fail.get(h).map(|e| (h.clone(), e.clone())))
                .collect()
        }
    }

    fn binding(hotkey: &str, enabled: bool) -> Binding {
        Binding {
            hotkey: hotkey.into(),
            enabled,
            action: Action::LaunchProgram { path: "x".into(), args: vec![] },
        }
    }

    fn profile(bindings: Vec<Binding>) -> Profile {
        Profile { schema_version: 1, bindings }
    }

    #[test]
    fn sync_registers_emergency_and_enabled_only() {
        let targets: TargetMap = Default::default();
        let backend = FakeBackend::default();
        let applied = Arc::clone(&backend.applied);
        let mut engine = HotkeyEngine::new(Box::new(backend), Arc::clone(&targets));

        engine.sync(
            "Ctrl+Alt+End",
            &profile(vec![binding("Ctrl + Alt + K", true), binding("Ctrl+X", false), binding("", true)]),
        );

        assert_eq!(
            applied.lock().unwrap()[0],
            vec![
                "ctrl+alt+end",
                "ctrl+alt+shift+end",
                "ctrl+alt+super+end",
                "ctrl+alt+shift+super+end",
                "ctrl+alt+k",
            ]
        );
        let map = targets.lock().unwrap();
        assert_eq!(map.get("ctrl+alt+end"), Some(&FireTarget::EmergencyStop));
        // supersets fire the stop even while a macro holds extra modifiers
        assert_eq!(map.get("ctrl+alt+shift+end"), Some(&FireTarget::EmergencyStop));
        assert!(matches!(map.get("ctrl+alt+k"), Some(FireTarget::Run(_))));
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn emergency_superset_expansion() {
        assert_eq!(emergency_supersets("f9"), vec!["f9", "ctrl+f9", "alt+f9", "ctrl+alt+f9", "shift+f9", "ctrl+shift+f9", "alt+shift+f9", "ctrl+alt+shift+f9", "super+f9", "ctrl+super+f9", "alt+super+f9", "ctrl+alt+super+f9", "shift+super+f9", "ctrl+shift+super+f9", "alt+shift+super+f9", "ctrl+alt+shift+super+f9"]);
        assert_eq!(
            emergency_supersets("ctrl+alt+shift+super+end"),
            vec!["ctrl+alt+shift+super+end"]
        );
    }

    #[test]
    fn duplicate_combo_is_conflict_first_wins() {
        let targets: TargetMap = Default::default();
        let mut engine = HotkeyEngine::new(Box::new(FakeBackend::default()), Arc::clone(&targets));

        engine.sync("Ctrl+Alt+End", &profile(vec![binding("Ctrl+Alt+K", true), binding("ctrl+alt+k", true)]));

        assert!(engine.error_for("CTRL+ALT+K").unwrap().contains("conflict"));
        // first binding stays active
        assert!(matches!(targets.lock().unwrap().get("ctrl+alt+k"), Some(FireTarget::Run(_))));
    }

    #[test]
    fn emergency_stop_survives_conflicting_binding() {
        let targets: TargetMap = Default::default();
        let mut engine = HotkeyEngine::new(Box::new(FakeBackend::default()), Arc::clone(&targets));

        engine.sync("Ctrl+Alt+End", &profile(vec![binding("Ctrl+Alt+End", true)]));

        assert!(engine.error_for("Ctrl+Alt+End").unwrap().contains("conflict"));
        assert_eq!(targets.lock().unwrap().get("ctrl+alt+end"), Some(&FireTarget::EmergencyStop));
    }

    #[test]
    fn backend_failure_excludes_target() {
        let targets: TargetMap = Default::default();
        let backend = FakeBackend {
            fail: HashMap::from([("ctrl+alt+k".to_string(), "taken by another app".to_string())]),
            ..Default::default()
        };
        let mut engine = HotkeyEngine::new(Box::new(backend), Arc::clone(&targets));

        engine.sync("Ctrl+Alt+End", &profile(vec![binding("Ctrl+Alt+K", true)]));

        assert_eq!(engine.error_for("Ctrl+Alt+K").unwrap(), "taken by another app");
        let map = targets.lock().unwrap();
        assert!(map.contains_key("ctrl+alt+end"));
        assert!(!map.contains_key("ctrl+alt+k"));
    }
}
