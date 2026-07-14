#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod bindings;
mod exec;
mod hotkey;
mod input;
mod keys;
mod macros;
mod persist;
mod settings;
mod tray;
mod window;

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};

fn fatal(msg: &str) -> ! {
    eprintln!("FATAL: {msg}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("KeyForge")
        .set_description(msg)
        .show();
    std::process::exit(1);
}

fn data_dir() -> PathBuf {
    let exe = std::env::current_exe()
        .unwrap_or_else(|e| fatal(&format!("Cannot locate the KeyForge executable: {e}")));
    exe.parent()
        .unwrap_or_else(|| fatal("Executable has no parent directory"))
        .join("keyforge_data")
}

fn bootstrap(data: &Path) {
    for sub in ["", "logs", "macros", "profiles", "plugins", "scripts"] {
        if let Err(e) = fs::create_dir_all(data.join(sub)) {
            fatal(&format!(
                "Cannot create {}: {e}\n\nKeyForge is portable and stores all data next to its \
                 executable. Move the app to a writable location and start it again.",
                data.display()
            ));
        }
    }
    let probe = data.join(".write_probe");
    if let Err(e) = fs::write(&probe, b"ok") {
        fatal(&format!(
            "{} is not writable: {e}\n\nKeyForge is portable and stores all data next to its \
             executable. Move the app to a writable location and start it again.",
            data.display()
        ));
    }
    let _ = fs::remove_file(&probe);
}

// Held for the whole process lifetime; the OS releases the lock on exit/crash.
fn acquire_single_instance_lock(data: &Path) -> File {
    let path = data.join("keyforge.lock");
    let file = File::create(&path)
        .unwrap_or_else(|e| fatal(&format!("Cannot create lock file {}: {e}", path.display())));
    match file.try_lock() {
        Ok(()) => file,
        // ponytail: second launch shows a dialog instead of focusing the running
        // instance; wire focus-existing in M5 when the windows/x11rb backends land.
        Err(TryLockError::WouldBlock) => fatal("KeyForge is already running."),
        Err(TryLockError::Error(e)) => fatal(&format!("Cannot lock {}: {e}", path.display())),
    }
}

fn init_logging(data: &Path) -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let file = tracing_appender::rolling::daily(data.join("logs"), "keyforge.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();
    guard
}

fn main() -> eframe::Result {
    let data = data_dir();
    bootstrap(&data);
    let _lock = acquire_single_instance_lock(&data);
    let _log_guard = init_logging(&data);
    tracing::info!(data_dir = %data.display(), version = env!("CARGO_PKG_VERSION"), "KeyForge starting");

    let settings = settings::Settings::load_or_create(&data.join("settings.json"));
    macros::seed_example(&data.join("macros"));

    // Outlives run_native; macro executions run here so they never block the UI.
    let runtime = tokio::runtime::Runtime::new()
        .unwrap_or_else(|e| fatal(&format!("Cannot start async runtime: {e}")));
    let dispatcher = exec::Dispatcher {
        handle: runtime.handle().clone(),
        executions: std::sync::Arc::default(),
        inputs: std::sync::Arc::new(input::Inputs::new()),
        wm: std::sync::Arc::new(window::NativeWindowManager),
        macros_dir: data.join("macros"),
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([640.0, 400.0])
            .with_title("KeyForge"),
        ..Default::default()
    };
    eframe::run_native(
        "KeyForge",
        options,
        Box::new(move |cc| Ok(Box::new(app::KeyForgeApp::new(cc, data, settings, dispatcher)))),
    )
}
