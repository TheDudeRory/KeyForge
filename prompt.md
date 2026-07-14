# Project: Keyforge — Portable Cross-Platform Hotkey & Automation Manager

Build a desktop application that lets me **graphically program global hotkeys** into automation sequences: manipulate windows, simulate keyboard/mouse input, switch audio devices, launch programs/files, run plugins, and **check conditions** (is a window open? is a device plugged in?) with branching logic. Think "AutoHotkey with a visual editor," cross-platform.

## Hard Requirements

1. **Target OSes: Windows 10/11 and Linux (X11).** macOS is explicitly out of scope. Wayland gets best-effort support later (see Milestone 11) — do not let Wayland complexity block X11 progress.
2. **Fully portable.** The app is a single executable. ALL state — settings, macros, profiles, logs, plugin registry — lives in a `keyforge_data/` directory **next to the executable**, never in `%APPDATA%`, `~/.config`, or the registry. If the executable's directory is not writable, show a clear error dialog at startup telling the user to move the app somewhere writable (do NOT silently fall back to a home directory — that breaks the portability contract).
3. **All persistence is human-readable JSON** (pretty-printed, stable key ordering) so files diff cleanly in git.
4. **Graphical programming.** No hand-written config required for normal use. Macros are built in a visual block editor (spec below).

## Architecture Decisions (already made — do not relitigate)

- **Language/UI: Rust with `eframe`/`egui`.** Chosen over Tauri deliberately: no WebView2/webkit2gtk runtime dependency, which is what makes true run-from-USB portability possible. Single static-ish binary per OS.
- **Async runtime: `tokio`.** Macro executions run as spawned tasks so long-running macros never block the UI or the hotkey listener.
- **Crates (starting set — verify current versions and swap if something is unmaintained):**
  - `global-hotkey` — system-wide hotkey registration (Win + X11)
  - `enigo` — keyboard/mouse simulation
  - `device_query` or `rdev` — input state queries and the macro recorder (Milestone 10)
  - `windows` crate — Win32 window management (EnumWindows, SetForegroundWindow, MoveWindow, SetWindowPos, etc.)
  - `x11rb` — Linux window management via EWMH (`_NET_ACTIVE_WINDOW`, `_NET_CLIENT_LIST`, `_NET_WM_STATE`, move/resize)
  - Audio device switching: on Windows use `windows` crate COM (`IPolicyConfig` / `IMMDeviceEnumerator`); on Linux shell out to `pactl` (PulseAudio/PipeWire-pulse) — detect availability at runtime
  - `sysinfo` — process list, system queries
  - `rhai` — embedded scripting for expressions and script-type plugins
  - `serde`/`serde_json`, `tracing` + `tracing-appender` (log to `keyforge_data/logs/`, rotating)
- **Macro model: a tree of steps, not a node graph.** The visual editor is a **Scratch-style nested block list**: a vertical sequence of action blocks, where control-flow blocks (If/Else, Loop, Wait-Until) contain nested child sequences. This was chosen over a node/wire graph because it is dramatically easier to make robust in egui, serializes cleanly to JSON, and maps 1:1 to execution semantics.
- **Every OS-specific capability sits behind a trait** (`WindowManager`, `InputSimulator`, `AudioDeviceManager`, `DeviceMonitor`) with a Windows impl and a Linux impl selected at compile time via `#[cfg]`. Shared logic never touches OS APIs directly. This is the single most important structural rule in the codebase.

## Data Model (define in Milestone 1, evolve additively)

```
keyforge_data/
  settings.json        // app settings, active profile, UI prefs
  profiles/
    default.json       // a profile = list of bindings
  macros/
    <uuid>.json        // one macro per file (clean diffs)
  plugins/
    <plugin-dir>/manifest.json
  scripts/             // .rhai user scripts
  logs/
```

- **Binding** = trigger → macro reference. Triggers: hotkey (modifiers + key), and later device/window events (Milestone 9).
- **Macro** = `{ id, name, description, steps: [Step] }`.
- **Step** = tagged enum: `{ type, params, children? }`. Params may contain literal values or `{ "expr": "..." }` Rhai expressions evaluated at runtime against macro variables.
- **Profiles**: named sets of bindings, switchable from the tray menu and via a "Switch Profile" action (so a hotkey can swap keymaps — e.g., a "gaming" vs "work" layer).
- Include a `schema_version` field in every JSON file from day one, with a migration hook, so old files never break.

## Step Catalog

Implement these step types. Each needs: execution logic, a parameter form in the editor, JSON (de)serialization, and a one-line human-readable summary for its collapsed block.

**Input**
- Send Keystroke (combo, e.g. Ctrl+Shift+T), Type Text (string, with per-char delay option), Hold Key / Release Key
- Mouse Move (absolute / relative / to-window-relative), Mouse Click (button, single/double), Mouse Drag (from→to), Scroll (direction, amount)

**Windows**
- Focus Window, Move/Resize Window (x, y, w, h; supports percentages of monitor), Minimize / Maximize / Restore / Close Window
- Toggle Always-On-Top, Move Window to Monitor N, Set Window Transparency (Windows only; no-op with warning on Linux)
- All window steps share one **WindowSelector** param object: match by title (exact/contains/regex), by process name, or by class — plus "currently focused window."

**Devices**
- Set Default Audio Output / Input (picked from a live-enumerated dropdown, stored by device name with fuzzy re-match at runtime since IDs change)
- Adjust Volume / Mute Toggle (master or per-app on Windows)

**System**
- Launch Program (path, args, working dir, optional "run hidden")
- Open File/Folder/URL (OS default handler)
- Run Shell Command (capture stdout/exit code into macro variables; configurable timeout)
- Clipboard: Set / Get-into-variable
- Show Notification (toast/tray balloon), Play Sound

**Control Flow**
- If / Else (condition, see below), Loop N Times, While (condition), Break
- Wait (fixed ms), **Wait Until (condition, poll interval, timeout, on-timeout branch)**
- Set Variable (name, value/expression), Stop Macro, Run Macro (call another macro — detect recursion, cap call depth at 16)
- Confirm Dialog (message; OK continues, Cancel stops) — cheap safety valve for destructive macros

**Conditions** (used by If/While/Wait-Until — this is the "check things" requirement, make it first-class)
- Window Exists (WindowSelector), Window Is Focused, Window Title Matches
- Process Is Running (name)
- **Device Is Connected** (USB — match by VID:PID or name substring; enumerate via `rusb` or sysfs on Linux, SetupAPI/`windows` crate on Windows)
- Audio Device Exists (name)
- File / Directory Exists (path)
- Pixel Color At (x, y, color, tolerance) — screen capture via `xcap` or equivalent
- Clipboard Contains (substring/regex)
- Variable Comparison (==, !=, <, >, contains)
- Time Is Between (HH:MM–HH:MM), Day of Week Is
- Rhai Expression (escape hatch: arbitrary boolean expression)
- Conditions compose with AND/OR/NOT groups in the condition editor UI.

## Visual Editor Spec (Milestone 7 — the heart of the app)

- Left panel: searchable **block palette**, grouped by the categories above. Click or drag to append.
- Center: the macro as a vertical stack of blocks. Control-flow blocks render as containers with indented child slots (Scratch-style). Support: drag-to-reorder (including into/out of containers), right-click → cut/copy/paste/duplicate/delete/**disable** (disabled steps are skipped and rendered dimmed — invaluable for debugging), multi-select with shift-click.
- Each block: icon, human-readable summary line ("Click left at (240, 800)"), expand-in-place to edit params. Param widgets must be type-appropriate: key-combo capture field (press keys to record), window picker (live list + "pick by clicking a window" crosshair mode), file picker, screen-position picker (crosshair overlay that also shows the pixel color under the cursor — doubles as the picker for Pixel Color conditions), device dropdowns, monitor picker.
- Undo/redo for all editor operations (keep an edit-history stack of macro snapshots; macros are small, don't over-engineer).
- **Test Run** button: executes the macro with the currently executing block highlighted in the editor, live variable values in a side panel, and step timings. Also a "step-through" mode (execute one block per click).
- Validation pass on save: unreachable steps after Stop, empty selectors, hotkey conflicts — shown as inline warnings, not save-blockers.

## Main UI Layout

- **Bindings tab**: table of hotkey → macro, per-profile; enable/disable toggles; conflict indicators (two bindings on the same combo in the same profile = error badge).
- **Macros tab**: macro library list → opens the editor.
- **Devices tab**: live view of connected audio + USB devices (this doubles as the debugging aid for writing device conditions — show exactly the names/IDs the matcher will see).
- **Log tab**: live tail of execution log with per-macro filtering.
- **Settings tab**: start with OS toggle (registry Run key / XDG autostart .desktop file — both must point at the portable exe's current path and be re-checked at startup in case the folder moved), start minimized, theme, emergency-stop hotkey config.
- Tray icon: pause/resume all hotkeys, switch profile, open window, quit. Closing the window minimizes to tray (setting to change this).

## Safety Rails (non-negotiable, implement early)

- **Emergency stop**: a reserved, always-registered hotkey (default `Ctrl+Alt+End`, configurable) that instantly cancels ALL running macros and releases any held keys/mouse buttons. Input-simulation steps must track held state so the stop can release them — a macro that dies holding Ctrl down makes the machine unusable.
- **Runaway guard**: per-macro max runtime (default 60s, per-macro override) and max loop iterations (default 10,000). Exceeding either cancels the macro and logs loudly.
- **Single instance**: lockfile in `keyforge_data/`; second launch focuses the existing instance.
- Every macro execution logs start/end/error with duration; step-level logging at debug level.

## Milestones

Each milestone must compile and run on **both** Windows and Linux before moving to the next. Where you can't test the other OS, keep the OS-specific code behind the trait boundary and stub it with a clear `todo!()`-free fallback that logs "unsupported."

1. **Skeleton.** eframe app with tab layout, tray icon, portable `keyforge_data/` bootstrap with writability check, settings.json load/save, logging, single-instance lock. Empty tabs OK.
2. **Hotkeys end-to-end.** Trait `HotkeyBackend` over `global-hotkey`; register/unregister at runtime; a hardcoded binding fires a hardcoded "Launch Program" action. Bindings tab with the table UI and a raw key-combo capture widget. Emergency-stop hotkey registered (it just logs for now).
3. **Macro engine.** Step/Macro data model with serde; recursive async executor with cancellation tokens; variables + Rhai expression evaluation; control-flow steps (If/Loop/While/Wait/Wait-Until/Set Variable/Stop/Run Macro); runaway guards; execution logging. Console-testable — no editor yet, load macros from hand-written JSON in `macros/`.
4. **Input simulation.** `InputSimulator` trait + enigo impl; keystroke/type/hold/mouse steps; held-input tracking wired into emergency stop.
5. **Window management.** `WindowManager` trait; Win32 impl + x11rb/EWMH impl; all window steps + window/process conditions; WindowSelector matching logic with unit tests against a fake backend.
6. **Devices & system steps.** Audio device enumeration/switching, USB device enumeration + Device-Is-Connected condition, clipboard, notifications, shell command with output capture, remaining conditions (pixel color, file exists, time).
7. **Visual editor.** Full spec above, minus test-run.
8. **Test run & debugging.** Highlighted execution, variable inspector, step-through, disable-step, validation warnings.
9. **Event triggers.** Beyond hotkeys: on-device-connected/disconnected, on-window-opened/closed (matching a WindowSelector), on-app-start, on-timer/schedule. These reuse the binding model (trigger → macro).
10. **Macro recorder.** Record keyboard/mouse into a step sequence (global listener via `rdev`); coalesce mouse moves; recorded steps drop into the editor for cleanup. Recording indicator overlay + stop hotkey.
11. **Plugins & polish.** Two plugin flavors: (a) **Rhai scripts** in `scripts/` exposed as a "Run Script" step with a host API (windows, input, clipboard, variables, http_get); (b) **executable plugins**: any exe/script in `plugins/<name>/` with a `manifest.json` declaring provided actions; invoked as a subprocess with JSON on stdin, JSON result on stdout, timeout enforced. Plus: macro/profile import-export (single .json bundle), first-run tour, README with per-OS build instructions, GitHub Actions CI building both targets. Document Wayland status honestly here: global hotkeys and input injection are restricted on Wayland; detect Wayland sessions at startup and show a banner recommending X11/XWayland, and investigate `evdev`/`uinput` (needs udev rule) as an opt-in path — but ship X11 support, don't chase Wayland perfection.

## Out of Scope — do not build

- macOS support (but don't gratuitously block it — the trait architecture is the concession)
- Node/wire graph editor, image recognition / OCR triggers, cloud sync, mobile remote, installer packages (portable zip only), localization, per-application hotkey contexts beyond profiles, DirectInput/game-injection tricks

## Windows-Specific Gotchas (handle, don't discover them in month two)

- Simulated input and window focus **do not reach elevated (admin) windows** from a non-elevated process. Detect the failure (SetForegroundWindow returns false / injection silently dropped), log it, and add a Settings note + optional "relaunch elevated" button with the tradeoff explained.
- `SetForegroundWindow` is restricted; use the standard workarounds (AttachThreadInput or the Alt-key nudge) inside the Windows `WindowManager` impl.
- Register hotkeys on a dedicated thread with a message pump if `global-hotkey` requires it; hotkey handling must never depend on the egui frame loop being awake.

## Working Rules

- Commit at every milestone boundary with a message summarizing what works.
- Unit-test the pure core hard (executor, condition evaluation, selector matching, serialization round-trips, expression evaluation) against fake trait impls; don't try to integration-test real input injection.
- If a chosen crate turns out to be dead or broken on one OS, pick a replacement, note it in `DECISIONS.md`, and keep moving — the trait boundary exists so this is cheap.
- Keep `DECISIONS.md` updated with any deviation from this spec and why.