# KeyForge — Execution Plan

Source spec: [prompt.md](prompt.md). Architecture is decided there (Rust + eframe/egui, tokio,
trait-per-OS-capability, JSON-in-`keyforge_data/`, Scratch-style block editor). This plan is the
build order and per-milestone deliverables. One commit minimum per milestone.

## Rules carried from spec

- Both OS targets (Windows 10/11, Linux X11) must **compile** at every milestone; untestable OS code goes behind traits with logging stubs.
- Portable: all state in `keyforge_data/` next to the exe. Writability check at startup, error dialog on failure.
- Every JSON file carries `schema_version`. Pretty-printed, stable ordering.
- Deviations from spec → `DECISIONS.md`.
- Unit-test the pure core (executor, conditions, selectors, serde round-trips) against fake trait impls.

## Milestones

- [x] **M1 Skeleton** — cargo project; eframe app with 5 tabs (Bindings/Macros/Devices/Log/Settings, empty);
      exe-relative `keyforge_data/` bootstrap + writability check + error dialog; `settings.json` load/save;
      tracing → rotating logs in `keyforge_data/logs/`; single-instance lock (std file lock);
      tray icon (Windows; Linux stub) with open/quit, close-to-tray.
- [ ] **M2 Hotkeys** — `HotkeyBackend` trait over `global-hotkey` on a dedicated thread; runtime (un)register;
      hardcoded binding → Launch Program; Bindings tab table + key-combo capture widget; emergency-stop hotkey (logs).
- [ ] **M3 Macro engine** — Step/Macro serde model; tokio recursive async executor + cancellation;
      variables + Rhai expressions; control-flow steps; runaway guards (60s / 10k iterations); execution logging.
      Driven by hand-written JSON in `macros/`. Heavy unit tests.
- [ ] **M4 Input simulation** — `InputSimulator` trait + enigo impl; keystroke/type/hold/mouse steps;
      held-input tracking → emergency stop releases everything.
- [ ] **M5 Window management** — `WindowManager` trait; `windows` crate impl (SetForegroundWindow workarounds,
      elevated-window detection) + `x11rb`/EWMH impl; all window steps + window/process conditions;
      WindowSelector matching w/ unit tests vs fake backend.
- [ ] **M6 Devices & system** — audio enumerate/switch (IPolicyConfig / `pactl`), USB Device-Is-Connected,
      clipboard, notifications, shell command w/ output capture, remaining conditions (pixel color, file, time).
- [ ] **M7 Visual editor** — block palette, nested block list, drag-reorder, cut/copy/paste/duplicate/disable,
      param widgets (key capture, window picker, position picker, device dropdowns), undo/redo, save validation.
- [ ] **M8 Test run & debugging** — live block highlight, variable inspector, step-through, inline validation warnings.
- [ ] **M9 Event triggers** — device connect/disconnect, window open/close, app-start, timer/schedule triggers.
- [ ] **M10 Macro recorder** — `rdev` global listener → step sequence; mouse-move coalescing; recording overlay + stop hotkey.
- [ ] **M11 Plugins & polish** — Rhai script plugins + exe plugins (JSON stdin/stdout); import/export bundle;
      first-run tour; README; GitHub Actions CI both targets; honest Wayland banner.

## Out of scope (spec §Out of Scope)

macOS, node graphs, OCR, cloud sync, installers, localization.
