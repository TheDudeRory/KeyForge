// Single typed wrapper around every Tauri invoke/listen call KeyForge makes.
// Every command listed here is registered in src-tauri/src/lib.rs's
// `invoke_handler`; the shapes mirror the Rust `Serialize` structs exactly.
// Macro CRUD + device IPC lives in components/MacroEditor.tsx (it owns the macro
// model types those calls carry) — everything else is here.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ── Portable persistence ─────────────────────────────────────────────────────
// The backend reads/writes keyforge.json next to the exe (src-tauri/src/state.rs).
export const loadState = (): Promise<string | null> => invoke("load_state");
export const saveState = (json: string): Promise<void> => invoke("save_state", { json });
export const statePath = (): Promise<string> => invoke("state_path");
export const backupCorruptState = (): Promise<string | null> => invoke("backup_corrupt_state");

// File helpers (log dumps out of the command palette).
export const writeText = (path: string, content: string): Promise<void> =>
  invoke("write_text", { path, content });
export const readText = (path: string): Promise<string> => invoke("read_text", { path });
export const appendText = (path: string, content: string): Promise<void> =>
  invoke("append_text", { path, content });
export const logsDir = (): Promise<string> => invoke("logs_dir");

/** The running build's version, shown in Settings → General. */
export const appVersion = (): Promise<string> => invoke("app_version");

// ── Global Hotkeys ───────────────────────────────────────────────────────────
// The Rust engine owns the profile JSON in the portable hotkeys/ dir next to the
// exe; these register combos with the OS. Actions: launch a program, run a shell
// command line, or fire a macro from the macro engine (Action::RunMacro in
// src-tauri/src/hotkeys/bindings.rs) by macro id. Input synthesis / window /
// audio live in the macro engine and are reachable only through run_macro, never
// as bare hotkey actions.
export type HotkeyAction =
  | { type: "launch_program"; path: string; args?: string[] }
  | { type: "run_command"; command: string }
  | { type: "run_macro"; id: string };

export interface HotkeyBinding {
  hotkey: string; // combo string, e.g. "Ctrl+Alt+K"
  enabled: boolean;
  action: HotkeyAction;
}

// `errors` maps a NORMALIZED combo (lowercased, no spaces) to why it is inactive
// (conflict with an earlier binding / invalid combo / the OS refused it).
// `emergency_stop` is the always-registered safety combo (hotkeys/bindings.rs
// DEFAULT_ESTOP), edited through hotkeysSetEstop rather than the binding list.
export interface HotkeyView {
  bindings: HotkeyBinding[];
  emergency_stop: string;
  errors: Record<string, string>;
}

export const hotkeysList = (): Promise<HotkeyView> => invoke("hotkeys_list");
export const hotkeysSave = (bindings: HotkeyBinding[]): Promise<HotkeyView> =>
  invoke("hotkeys_save", { bindings });
/** Rebind the emergency stop. Empty/unparseable falls back to the default in
 *  Rust rather than leaving the safety stop unregistered. */
export const hotkeysSetEstop = (combo: string): Promise<HotkeyView> =>
  invoke("hotkeys_set_estop", { combo });

// ── Macro test runs ──────────────────────────────────────────────────────────
// Macro editor play button — run the DRAFT being edited without saving it.
// `macro` is the editor's macro JSON (same shape macros_save takes). Both
// starters return a run id; progress arrives as "macro-test-step" events and
// exactly one "macro-test-done" per run. Cancel stops that run only (unlike
// macros_stop_all, which is the emergency stop for every running macro).
// Sub-macros in a `run_macro` step still resolve to their last SAVED version.
export type MacroTestStatus = "start" | "ok" | "skipped" | "error";
export interface MacroTestStep {
  run_id: number;
  index: number; // top-level step index in macro.steps
  total: number;
  summary: string; // human-readable step label
  status: MacroTestStatus;
  error?: string; // set when status === "error"
}
export interface MacroTestDone {
  run_id: number;
  status: "completed" | "stopped" | "error";
  error: string | null;
}
export const macrosTestRun = (macro: unknown): Promise<number> =>
  invoke("macros_test_run", { macro });
export const macrosTestStep = (macro: unknown, index: number): Promise<number> =>
  invoke("macros_test_step", { macro, index });
export const macrosTestCancel = (runId: number): Promise<boolean> =>
  invoke("macros_test_cancel", { runId });
export const onMacroTestStep = (cb: (s: MacroTestStep) => void): Promise<UnlistenFn> =>
  listen<MacroTestStep>("macro-test-step", (e) => cb(e.payload));
export const onMacroTestDone = (cb: (d: MacroTestDone) => void): Promise<UnlistenFn> =>
  listen<MacroTestDone>("macro-test-done", (e) => cb(e.payload));

// ── App-level global shortcuts ───────────────────────────────────────────────
// Owned by lib.rs, distinct from the user's Global Hotkeys above: the
// summon/hide toggle and the screenshot trigger. `screenshot` is stored and
// validated but NOT registered with the OS — KeyForge has no capture of its own
// (see the AppShortcuts doc comment in src-tauri/src/lib.rs). Editable from the
// Keybinds settings category; persisted next to the exe.
export interface AppShortcuts {
  summon: string; // combo, e.g. "CmdOrCtrl+Alt+Backquote"
  screenshot: string; // combo, e.g. "CmdOrCtrl+Alt+S"
}
export const appShortcutsGet = (): Promise<AppShortcuts> => invoke("app_shortcuts_get");
export const appShortcutsSet = (s: AppShortcuts): Promise<void> =>
  invoke("app_shortcuts_set", { summon: s.summon, screenshot: s.screenshot });
