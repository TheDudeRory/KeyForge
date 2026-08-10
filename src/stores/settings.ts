// The UI store: appearance settings, the in-app keymap, and the transient
// open/closed flags for the overlays (Settings, command palette, macro editor).
// Anything durable here is mirrored to keyforge.json next to the exe by
// lib/persist.ts; the open/closed flags are deliberately NOT persisted.
import { create } from "zustand";
import {
  DEFAULT_SETTINGS, loadPersisted, savePersistedDebounced, SCHEMA,
  type Persisted, type Settings, type TabId,
} from "../lib/persist";
import { DEFAULT_KEYMAP, type Keymap } from "../lib/keymap";

interface UiState {
  // persisted
  settings: Settings;
  keymap: Keymap;
  // transient
  hydrated: boolean;
  tab: TabId;
  settingsOpen: boolean;
  paletteOpen: boolean;

  hydrate: () => Promise<void>;
  updateSettings: (patch: Partial<Settings>) => void;
  setKeybind: (action: string, combo: string) => void;
  resetKeymap: () => void;
  setTab: (tab: TabId) => void;
  openSettings: (open: boolean) => void;
  openPalette: (open: boolean) => void;
}

const snapshot = (s: UiState): Persisted => ({
  schemaVersion: SCHEMA,
  settings: s.settings,
  keymap: s.keymap,
});

/** Save the durable slice after any edit. Reads the state at flush time, so a
 *  burst of edits costs one write of the final value. */
const persist = () => savePersistedDebounced(() => snapshot(useUi.getState()));

export const useUi = create<UiState>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  keymap: DEFAULT_KEYMAP,
  hydrated: false,
  tab: DEFAULT_SETTINGS.startTab,
  settingsOpen: false,
  paletteOpen: false,

  hydrate: async () => {
    const data = await loadPersisted();
    if (data) set({ settings: data.settings, keymap: data.keymap, tab: data.settings.startTab });
    set({ hydrated: true });
  },

  updateSettings: (patch) => {
    set({ settings: { ...get().settings, ...patch } });
    persist();
  },

  setKeybind: (action, combo) => {
    set({ keymap: { ...get().keymap, [action]: combo } });
    persist();
  },

  resetKeymap: () => {
    set({ keymap: { ...DEFAULT_KEYMAP } });
    persist();
  },

  setTab: (tab) => set({ tab }),
  openSettings: (settingsOpen) => set({ settingsOpen }),
  openPalette: (paletteOpen) => set({ paletteOpen }),
}));
