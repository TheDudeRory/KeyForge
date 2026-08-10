//! KeyForge — global hotkeys and macro automation.
//!
//! The Rust side is three modules: `hotkeys` (the OS shortcut registry + the
//! IPC surface the UI talks to), `macros` (the execution engine those hotkeys
//! fire), and `state` (portable file IO next to the exe). Everything the
//! frontend can call is registered in `run`'s `invoke_handler` below.

mod childenv;
mod hotkeys;
pub mod macros;
mod state;

/// The running build's version, so Settings can show what the user is on.
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Pin GTK/WebKit to X11 on Linux, before GTK initialises.
///
/// WebKitGTK 2.52 plus the appindicator tray die with "Error 71 (Protocol
/// error) dispatching to Wayland display" before the window ever shows, and
/// the DMA-BUF renderer is the other half of that crash on Mesa.
///
/// CONSEQUENCE: X11 or XWayland is REQUIRED — on a pure-Wayland system with no
/// XWayland the app cannot start. Set `GDK_BACKEND` yourself to override (both
/// variables are only applied when unset). `macros/window.rs` and
/// `macros/sys.rs` are X11-only for the same reason.
///
/// Whatever we set here is recorded so `childenv::strip_webview_env` can keep
/// it out of the programs hotkeys and macros launch — those are the user's
/// processes, not ours, and must see the user's real session.
pub fn apply_linux_webview_env() {
    #[cfg(target_os = "linux")]
    {
        let mut ours = Vec::new();
        for (key, val) in [("GDK_BACKEND", "x11"), ("WEBKIT_DISABLE_DMABUF_RENDERER", "1")] {
            if std::env::var_os(key).is_none() {
                std::env::set_var(key, val);
                ours.push(key.to_string());
            }
        }
        childenv::record_ours(ours);
    }
}

/// Toggle the main window (for the global summon/hide hotkey).
fn toggle_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

/// Default app-level global shortcuts. These are owned by lib.rs, distinct from
/// the user's Global Hotkeys in the `hotkeys` module, and are editable from the
/// Keybinds settings category via `app_shortcuts_get`/`app_shortcuts_set`.
const SCREENSHOT_SHORTCUT: &str = "CmdOrCtrl+Alt+S";
const SUMMON_SHORTCUT: &str = "CmdOrCtrl+Alt+Backquote";

/// The app-level global shortcuts, persisted next to the exe so a rebind
/// survives restarts (portable, same dir as `keyforge.json`).
///
/// `screenshot` is stored and validated but NOT registered with the OS: KeyForge
/// has no capture of its own (that lived in the app this engine was lifted
/// from), and grabbing a system-wide combo that does nothing would only steal it
/// from the user's real screenshot tool. The field stays so the setting — and
/// the UI that edits it — survives until something claims it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct AppShortcuts {
    summon: String,
    screenshot: String,
}

impl Default for AppShortcuts {
    fn default() -> Self {
        Self { summon: SUMMON_SHORTCUT.into(), screenshot: SCREENSHOT_SHORTCUT.into() }
    }
}

/// Process-wide current values, read by the global-shortcut handler.
fn app_shortcuts_cell() -> &'static std::sync::Mutex<AppShortcuts> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<AppShortcuts>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(AppShortcuts::default()))
}

fn app_shortcuts_file() -> Result<std::path::PathBuf, String> {
    Ok(state::state_dir()?.join("app-shortcuts.json"))
}

fn load_app_shortcuts() -> AppShortcuts {
    app_shortcuts_file()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Current app-level global shortcuts (summon/hide + screenshot), for the UI.
#[tauri::command]
fn app_shortcuts_get() -> AppShortcuts {
    app_shortcuts_cell().lock().unwrap().clone()
}

/// Rebind the app-level global shortcuts: unregister the old summon combo,
/// register the new one with the OS, persist both, and update the live value the
/// handler reads. The screenshot combo is validated and stored only — see
/// `AppShortcuts`.
#[tauri::command]
fn app_shortcuts_set(app: tauri::AppHandle, summon: String, screenshot: String) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    use std::str::FromStr;
    Shortcut::from_str(&screenshot).map_err(|e| format!("screenshot '{screenshot}': {e}"))?;
    let gs = app.global_shortcut();
    {
        let cur = app_shortcuts_cell().lock().unwrap();
        if let Ok(sc) = cur.summon.parse::<Shortcut>() {
            let _ = gs.unregister(sc);
        }
    }
    gs.register(summon.as_str()).map_err(|e| format!("summon '{summon}': {e}"))?;
    let next = AppShortcuts { summon, screenshot };
    if let Ok(p) = app_shortcuts_file() {
        let _ = std::fs::write(p, serde_json::to_string_pretty(&next).unwrap_or_default());
    }
    *app_shortcuts_cell().lock().unwrap() = next;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Single-instance must be registered first: a second launch focuses the
        // existing window instead of opening another. Two KeyForge processes
        // would also fight over the same global shortcut registrations.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    // Emergency stop first (safety): if this combo (or one of
                    // its modifier supersets) is the estop, cancel all macros +
                    // release held input, and swallow the event.
                    if hotkeys::handle_estop(app, shortcut) {
                        return;
                    }
                    // Otherwise it is either one of the user's Global Hotkeys or
                    // the summon/hide toggle — nothing else is registered.
                    if !hotkeys::dispatch(app, shortcut) {
                        toggle_main_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            use tauri::Manager;
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            // App-level global shortcuts, from the persisted app-shortcuts.json
            // if present, else the defaults. Editable at runtime via
            // app_shortcuts_set (Keybinds settings category).
            let app_sc = load_app_shortcuts();
            let _ = app.global_shortcut().register(app_sc.summon.as_str());
            *app_shortcuts_cell().lock().unwrap() = app_sc;
            // Register the user's Global Hotkeys from the persisted
            // hotkeys/default.json profile, and start the macro engine.
            hotkeys::init(app.handle());
            // asset: protocol scope. The config ships an EMPTY static scope, so
            // the webview can read nothing by default. The portable dirs sit
            // next to the exe and their paths are only known at runtime
            // (state::state_dir = current_exe().parent()), so allow them here.
            if let Ok(dir) = state::screenshots_dir() {
                let _ = app.asset_protocol_scope().allow_directory(&dir, true);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            state::load_state,
            state::save_state,
            state::state_path,
            state::write_text,
            state::read_text,
            state::append_text,
            state::logs_dir,
            state::screenshots_dir,
            state::backup_corrupt_state,
            hotkeys::hotkeys_list,
            hotkeys::hotkeys_save,
            hotkeys::hotkeys_set_estop,
            hotkeys::macros_list,
            hotkeys::macros_run,
            hotkeys::macros_stop_all,
            hotkeys::macros_get,
            hotkeys::macros_save,
            hotkeys::macros_delete,
            hotkeys::macros_test_run,
            hotkeys::macros_test_step,
            hotkeys::macros_test_cancel,
            hotkeys::devices_audio,
            hotkeys::devices_audio_sessions,
            hotkeys::set_app_volume,
            hotkeys::devices_usb,
            app_shortcuts_get,
            app_shortcuts_set,
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
