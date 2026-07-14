# Decisions & Deviations

- **M1: single-instance = std `File::try_lock`** (stable since Rust 1.89) on `keyforge_data/keyforge.lock` — no crate needed, OS releases the lock on crash. Second launch shows an "already running" dialog instead of focusing the running instance; focus-existing will be wired in M5 when the `windows`/`x11rb` backends exist.
- **M1: no Linux tray icon yet.** `tray-icon` on Linux drags in gtk and needs its own event-loop thread; deferred to M11 polish. On Linux the close button quits (Settings tab says so). The `tray-icon` dependency is Windows-only in Cargo.toml.
- **M2: hotkey manager rides winit's message pump** instead of a dedicated pump thread. `GlobalHotKeyManager` is created on the main thread; winit keeps pumping messages even while the window is hidden, so firing never waits on an egui repaint. Upgrade path (dedicated thread) noted in `src/hotkey.rs` if winit's loop ever proves blocking.
- **M2: hotkeys stored as strings** ("Ctrl+Alt+K") parsed by global-hotkey's own parser — no custom key model. The capture widget emits parser-compatible names (pinned by the `captured_names_parse` test). Super/Win modifier can't be captured (egui doesn't report it) but parses if hand-typed in JSON.
- **M2: bindings carry an inline `launch_program` action** until macros exist; M3 switches bindings to macro references.
- **M2: conflict policy** — duplicate combos badge the later row, first registrant stays active; the emergency stop registers first so it always wins.
- **eframe 0.35 API**: `App::update` was split into `logic()` (no painting) + `ui(&mut Ui)`, and `TopBottomPanel`/`SidePanel` merged into `egui::Panel`. Code follows the new API.
