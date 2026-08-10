import { useCallback, useEffect, useState } from "react";
import HotkeyManager from "./components/HotkeyManager";
import DevicesTab from "./components/DevicesTab";
import Palette from "./components/Palette";
import SettingsModal from "./components/SettingsModal";
import { MacroEditor, macrosDelete, macrosList, macrosRun, macrosStopAll, type MacroInfo } from "./components/MacroEditor";
import { useUi } from "./stores/settings";
import { actionFor } from "./lib/keymap";
import type { TabId } from "./lib/persist";
import "./components/macros.css";
import "./App.css";

// The whole window: a tab bar over the three panels (Hotkeys / Macros /
// Devices), the Settings overlay, and the command palette. The macro library is
// owned here because two panels need it — the Macros list and the RunMacro
// binding picker in Hotkeys.

const TABS: [TabId, string][] = [
  ["hotkeys", "Hotkeys"],
  ["macros", "Macros"],
  ["devices", "Devices"],
];

export default function App() {
  const tab = useUi((s) => s.tab);
  const setTab = useUi((s) => s.setTab);
  const hydrated = useUi((s) => s.hydrated);
  const hydrate = useUi((s) => s.hydrate);
  const theme = useUi((s) => s.settings.theme);
  const uiScale = useUi((s) => s.settings.uiScale);
  const confirmOnDelete = useUi((s) => s.settings.confirmOnDelete);

  const [macros, setMacros] = useState<MacroInfo[]>([]);
  // Bumped to make the Hotkeys panel re-read the profile from Rust.
  const [reloadKey, setReloadKey] = useState(0);
  // Macro editor overlay: undefined = closed, null = new macro, string = edit id.
  const [editing, setEditing] = useState<string | null | undefined>(undefined);

  useEffect(() => { hydrate(); }, [hydrate]);

  const reloadMacros = useCallback(
    () => macrosList().then(setMacros).catch(() => setMacros([])),
    [],
  );
  const reloadAll = useCallback(() => { reloadMacros(); setReloadKey((n) => n + 1); }, [reloadMacros]);
  useEffect(() => { reloadMacros(); }, [reloadMacros]);

  // Theme + zoom are applied to the document, so the modals (which portal to
  // fixed position, outside the app shell) inherit them too.
  useEffect(() => { document.documentElement.dataset.theme = theme; }, [theme]);
  useEffect(() => { document.documentElement.style.fontSize = `${16 * uiScale}px`; }, [uiScale]);

  // Central key handler for the in-app keymap. Capture phase so a binding still
  // fires from inside an input, EXCEPT while a text field has focus and the
  // combo would be a normal edit — the keymap is all Ctrl/Alt combos, so the
  // only guard needed is the macro editor's own capture fields (KeyPicker stops
  // propagation itself).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const action = actionFor(useUi.getState().keymap, e);
      if (!action) return;
      const s = useUi.getState();
      switch (action) {
        case "palette": s.openPalette(!s.paletteOpen); break;
        case "settings": s.openSettings(!s.settingsOpen); break;
        case "hotkeysTab": s.setTab("hotkeys"); break;
        case "macrosTab": s.setTab("macros"); break;
        case "devicesTab": s.setTab("devices"); break;
        case "newMacro": s.setTab("macros"); setEditing(null); break;
        case "reload": reloadAll(); break;
        case "stopAll": macrosStopAll().catch(() => {}); break;
        default: return;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [reloadAll]);

  const removeMacro = (m: MacroInfo) => {
    if (confirmOnDelete && !confirm(`Delete macro "${m.name}"?`)) return;
    macrosDelete(m.id).then(reloadMacros).catch(() => {});
  };

  return (
    <div className="kf-app">
      <div className="mac-tabs">
        {TABS.map(([id, text]) => (
          <button key={id} className={`mac-tab${tab === id ? " active" : ""}`} onClick={() => setTab(id)}>{text}</button>
        ))}
        <div style={{ flex: 1 }} />
        <button className="mac-btn estop" style={{ margin: "6px 4px" }} title="Cancel every running macro and release held keys/mouse"
          onClick={() => macrosStopAll().catch(() => {})}>■ Stop all</button>
        <button className="mac-btn" style={{ margin: "6px 8px 6px 0" }} title="Settings"
          onClick={() => useUi.getState().openSettings(true)}>⚙ Settings</button>
      </div>

      <div className="kf-body">
        {!hydrated ? null : tab === "hotkeys" ? (
          <HotkeyManager macros={macros} reloadKey={reloadKey} />
        ) : tab === "macros" ? (
          <div className="mac-lib">
            <div className="mac-lib-toolbar">
              <button className="mac-btn primary" onClick={() => setEditing(null)}>＋ New macro</button>
              <button className="mac-btn" onClick={reloadMacros}>↻ Reload</button>
              <div style={{ flex: 1 }} />
              <span style={{ fontSize: 12, color: "var(--muted)" }}>{macros.length} macro{macros.length === 1 ? "" : "s"}</span>
            </div>
            <div className="mac-lib-list">
              {macros.length === 0 && <div style={{ padding: 16, color: "var(--muted)", fontSize: 13 }}>No macros yet. Click <b>New macro</b> to build one with the visual editor.</div>}
              {macros.map((m) => (
                <div className="mac-lib-row" key={m.id} onDoubleClick={() => setEditing(m.id)}>
                  <div style={{ flex: 1, overflow: "hidden" }}>
                    <div className="mac-lib-name">{m.name}</div>
                    {m.description && <div className="mac-lib-desc">{m.description}</div>}
                  </div>
                  <div className="mac-lib-actions">
                    <button className="mac-btn tiny" title="Run now" onClick={() => macrosRun(m.id).catch(() => {})}>▶ Run</button>
                    <button className="mac-btn tiny" onClick={() => setEditing(m.id)}>Edit</button>
                    <button className="mac-btn danger tiny" onClick={() => removeMacro(m)}>Delete</button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        ) : (
          <DevicesTab active={tab === "devices"} />
        )}
      </div>

      {editing !== undefined && (
        <MacroEditor macroId={editing} onClose={() => setEditing(undefined)} onSaved={reloadMacros} />
      )}
      <Palette macros={macros} onNewMacro={() => { setTab("macros"); setEditing(null); }} onReload={reloadAll} />
      <SettingsModal />
    </div>
  );
}
