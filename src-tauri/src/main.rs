// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Must run before GTK/WebKit initialise — see the doc comment for what it
    // works around and why X11/XWayland is required on Linux.
    keyforge_lib::apply_linux_webview_env();
    keyforge_lib::run()
}
