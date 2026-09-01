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

/// Toggle the main window (tray icon / single-instance handler).
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

/// What the window's close button and minimise do. Owned by the frontend
/// (keyforge.json) and pushed here with `tray_prefs_set` on every change,
/// because only Rust can intercept the window events. Defaults are "act like a
/// normal window" — until the UI has hydrated, closing really does quit.
#[derive(Clone, Copy, Default)]
struct TrayPrefs {
    close_to_tray: bool,
    minimize_to_tray: bool,
}

fn tray_prefs_cell() -> &'static std::sync::Mutex<TrayPrefs> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<TrayPrefs>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(TrayPrefs::default()))
}

#[tauri::command]
fn tray_prefs_set(close_to_tray: bool, minimize_to_tray: bool) {
    *tray_prefs_cell().lock().unwrap() = TrayPrefs { close_to_tray, minimize_to_tray };
}

/// The tray icon: click to show/hide, right-click for the menu. Always built —
/// it is the only way back to a window that was closed to the tray, and the only
/// way out of the app once closing no longer quits.
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "Show / hide window", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop all macros", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit KeyForge", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &stop, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("KeyForge")
        .menu(&menu)
        // Left click toggles the window; the menu is right-click only.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => toggle_main_window(app),
            "stop" => {
                hotkeys::stop_all(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
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
                    // Otherwise dispatch to the user's Global Hotkeys.
                    hotkeys::dispatch(app, shortcut);
                })
                .build(),
        )
        .setup(|app| {
            use tauri::Manager;
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
            // Never fatal: a desktop with no tray host (bare X session, some
            // Wayland setups) must still get its window.
            if let Err(e) = build_tray(app.handle()) {
                eprintln!("keyforge: tray icon unavailable: {e}");
            }
            Ok(())
        })
        .on_window_event(|win, event| {
            let prefs = *tray_prefs_cell().lock().unwrap();
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } if prefs.close_to_tray => {
                    api.prevent_close();
                    let _ = win.hide();
                }
                // Tauri has no "minimised" event: a minimise arrives as a resize
                // whose window then reports is_minimized(). Unminimise before
                // hiding, or the window comes back from the tray still minimised.
                tauri::WindowEvent::Resized(_) if prefs.minimize_to_tray => {
                    if win.is_minimized().unwrap_or(false) {
                        let _ = win.unminimize();
                        let _ = win.hide();
                    }
                }
                _ => {}
            }
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
            tray_prefs_set,
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
