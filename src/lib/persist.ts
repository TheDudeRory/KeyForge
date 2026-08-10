// Portable persistence for the UI's own settings. Backed by keyforge.json next
// to the exe through the Rust load_state/save_state commands (src-tauri/src/state.rs).
//
// What lives here is ONLY what the frontend owns: the appearance settings and
// the in-app keymap. The hotkey profile (hotkeys/default.json) and the macro
// library (macros/*.json) are owned by Rust and are never written from here.
import { loadState, saveState, backupCorruptState } from "./ipc";
import { DEFAULT_KEYMAP, type Keymap } from "./keymap";

export type Theme = "dark" | "light";
export type TabId = "hotkeys" | "macros" | "devices";

export interface Settings {
  theme: Theme;
  uiScale: number; // whole-interface zoom factor, 1 = 100%
  startTab: TabId; // which tab the window opens on
  confirmOnDelete: boolean; // ask before deleting a macro / hotkey
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "dark",
  uiScale: 1,
  startTab: "hotkeys",
  confirmOnDelete: true,
};

export interface Persisted {
  schemaVersion: number;
  settings: Settings;
  keymap: Keymap;
}

export const SCHEMA = 1;

/** schemaVersion 1 is current; future migrations branch here. */
function migrate(data: Persisted): Persisted {
  return data;
}

/** Fill in anything a hand-edited or older file left out, so one missing key
 *  cannot leave a control bound to `undefined`. */
export function withDefaults(data: Partial<Persisted> | null): Persisted {
  return {
    schemaVersion: SCHEMA,
    settings: { ...DEFAULT_SETTINGS, ...(data?.settings ?? {}) },
    keymap: { ...DEFAULT_KEYMAP, ...(data?.keymap ?? {}) },
  };
}

export async function loadPersisted(): Promise<Persisted | null> {
  let raw: string | null;
  try {
    raw = await loadState();
  } catch {
    return null; // not under Tauri (plain browser) or IO error
  }
  if (!raw) return null;
  try {
    const data = JSON.parse(raw) as Persisted;
    if (data?.schemaVersion !== SCHEMA) return null;
    return migrate(withDefaults(data));
  } catch {
    // Unparseable: move it aside as keyforge.json.corrupt-<ts> and start fresh.
    await backupCorruptState().catch(() => {});
    return null;
  }
}

let timer: ReturnType<typeof setTimeout> | undefined;
/** Debounced save; ignores errors (e.g. when running in a plain browser). */
export function savePersistedDebounced(build: () => Persisted, delay = 400): void {
  clearTimeout(timer);
  timer = setTimeout(() => {
    saveState(JSON.stringify(build())).catch(() => {});
  }, delay);
}
