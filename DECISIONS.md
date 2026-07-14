# Decisions & Deviations

- **M1: single-instance = std `File::try_lock`** (stable since Rust 1.89) on `keyforge_data/keyforge.lock` — no crate needed, OS releases the lock on crash. Second launch shows an "already running" dialog instead of focusing the running instance; focus-existing will be wired in M5 when the `windows`/`x11rb` backends exist.
- **M1: no Linux tray icon yet.** `tray-icon` on Linux drags in gtk and needs its own event-loop thread; deferred to M11 polish. On Linux the close button quits (Settings tab says so). The `tray-icon` dependency is Windows-only in Cargo.toml.
- **eframe 0.35 API**: `App::update` was split into `logic()` (no painting) + `ui(&mut Ui)`, and `TopBottomPanel`/`SidePanel` merged into `egui::Panel`. Code follows the new API.
