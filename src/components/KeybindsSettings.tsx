import { type CSSProperties, useEffect, useState } from "react";
import { useUi } from "../stores/settings";
import { KEYBIND_ACTIONS, eventToCombo, normalizeCombo } from "../lib/keymap";
import { appShortcutsGet, appShortcutsSet, hotkeysList, hotkeysSetEstop, type AppShortcuts } from "../lib/ipc";

// Keybinds settings category (registry entry in SettingsModal.tsx). Edits the
// three layers a combo can live in, in order of reach:
//   1. the persisted in-app `keymap` (window-focused actions, handled in App.tsx)
//   2. the OS-level app shortcuts owned by Rust (summon/hide + screenshot)
//   3. the emergency stop, always registered so it can kill a running macro
// The same keymap is what the central key handler reads, so nothing drifts.

const label: CSSProperties = { fontSize: 12, color: "var(--muted)" };
const row: CSSProperties = { display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, padding: "6px 0", borderBottom: "1px solid var(--border)" };
const field: CSSProperties = { background: "var(--panel-2)", color: "var(--text)", border: "1px solid var(--border)", borderRadius: 4, padding: "4px 7px", fontSize: 12.5 };
const kbd: CSSProperties = { background: "var(--border)", border: "1px solid var(--border)", borderRadius: 4, padding: "1px 7px", fontSize: 12, color: "var(--text)", whiteSpace: "nowrap" };

export default function KeybindsSettings() {
  const keymap = useUi((s) => s.keymap);
  const setKeybind = useUi((s) => s.setKeybind);
  const resetKeymap = useUi((s) => s.resetKeymap);
  const [capturing, setCapturing] = useState<string | null>(null);

  // While capturing, the next real combo keydown is written to the bound action.
  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setCapturing(null);
      const combo = eventToCombo(e);
      if (!combo) return; // modifier-only / unsupported key: keep listening
      setKeybind(capturing, combo);
      setCapturing(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, setKeybind]);

  // Conflict badges: a normalized combo bound to more than one action.
  const counts: Record<string, number> = {};
  for (const [, combo] of Object.entries(keymap)) {
    const n = normalizeCombo(combo);
    counts[n] = (counts[n] ?? 0) + 1;
  }

  return (
    <div>
      <p style={{ margin: "0 0 10px", fontSize: 11, color: "var(--muted)" }}>
        Click <b>Rebind</b>, then press the key combination. Esc cancels. Conflicts (the same
        combo bound twice) are flagged — the first matching action wins. These work while
        KeyForge itself is focused.
      </p>

      {KEYBIND_ACTIONS.map(([action, text]) => {
        const combo = keymap[action] ?? "";
        const conflict = combo && counts[normalizeCombo(combo)] > 1;
        return (
          <div style={row} key={action}>
            <span style={label}>
              {text}
              {conflict ? <span style={{ color: "#e0a06c", fontSize: 10, marginLeft: 6 }}>conflict</span> : null}
            </span>
            <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <kbd style={kbd}>{capturing === action ? "Press keys…" : combo || "—"}</kbd>
              <button onClick={() => setCapturing(action)} style={{ ...field, cursor: "pointer" }}>Rebind</button>
            </span>
          </div>
        );
      })}

      <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 10 }}>
        <button onClick={() => resetKeymap()} style={{ ...field, cursor: "pointer" }}>Reset to defaults</button>
      </div>

      <GlobalShortcuts />
      <EmergencyStop />
    </div>
  );
}

// The two OS-level app shortcuts live in Rust (editable via app_shortcuts_set).
// They use the plugin's combo syntax (e.g. "CmdOrCtrl+Alt+S"), so they are edited
// as text and applied on Save rather than key-captured.
function GlobalShortcuts() {
  const [sc, setSc] = useState<AppShortcuts | null>(null);
  const [msg, setMsg] = useState("");

  useEffect(() => {
    appShortcutsGet().then(setSc).catch(() => setSc(null));
  }, []);

  if (!sc) {
    return (
      <p style={{ marginTop: 16, fontSize: 11, color: "var(--muted)" }}>
        System-wide global shortcuts are unavailable (no Tauri backend).
      </p>
    );
  }

  const save = async () => {
    setMsg("Applying…");
    try {
      await appShortcutsSet(sc);
      setMsg("Saved.");
    } catch (e) {
      setMsg(`Failed: ${e}`);
    }
  };

  return (
    <div style={{ marginTop: 16, paddingTop: 8, borderTop: "1px solid var(--border)" }}>
      <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text)", marginBottom: 4 }}>Global (system-wide)</div>
      <p style={{ margin: "0 0 8px", fontSize: 11, color: "var(--muted)" }}>
        Registered with the OS so they work while other apps are focused. Use the plugin syntax,
        e.g. <code>CmdOrCtrl+Alt+S</code>.
      </p>
      <div style={row}>
        <span style={label}>Show / hide window</span>
        <input style={{ ...field, width: 200 }} value={sc.summon} onChange={(e) => setSc({ ...sc, summon: e.target.value })} />
      </div>
      <div style={row}>
        <span style={label}>Screenshot combo <span style={{ fontSize: 10 }}>(stored only — KeyForge has no capture)</span></span>
        <input style={{ ...field, width: 200 }} value={sc.screenshot} onChange={(e) => setSc({ ...sc, screenshot: e.target.value })} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, justifyContent: "flex-end", marginTop: 10 }}>
        {msg && <span style={{ fontSize: 11, color: "var(--muted)" }}>{msg}</span>}
        <button onClick={save} style={{ ...field, cursor: "pointer", background: "var(--accent)", color: "#fff", borderColor: "var(--accent)" }}>Apply global shortcuts</button>
      </div>
    </div>
  );
}

// The emergency stop is part of the hotkey profile, not the app shortcuts: Rust
// registers it AND its modifier supersets so it still fires while a macro holds
// extra modifiers down. Blank falls back to the built-in default rather than
// leaving the safety stop unregistered.
function EmergencyStop() {
  const [combo, setCombo] = useState<string | null>(null);
  const [msg, setMsg] = useState("");

  useEffect(() => {
    hotkeysList().then((v) => setCombo(v.emergency_stop)).catch(() => setCombo(null));
  }, []);

  if (combo == null) return null;

  const save = async () => {
    setMsg("Applying…");
    try {
      const v = await hotkeysSetEstop(combo);
      setCombo(v.emergency_stop);
      setMsg("Saved.");
    } catch (e) {
      setMsg(`Failed: ${e}`);
    }
  };

  return (
    <div style={{ marginTop: 16, paddingTop: 8, borderTop: "1px solid var(--border)" }}>
      <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text)", marginBottom: 4 }}>Emergency stop</div>
      <p style={{ margin: "0 0 8px", fontSize: 11, color: "var(--muted)" }}>
        Cancels every running macro and releases any keys or mouse buttons a macro is holding.
        Registered with its modifier supersets so it fires mid-macro. Leave blank to restore the
        default.
      </p>
      <div style={row}>
        <span style={label}>Stop all macros</span>
        <input style={{ ...field, width: 200 }} value={combo} onChange={(e) => setCombo(e.target.value)} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, justifyContent: "flex-end", marginTop: 10 }}>
        {msg && <span style={{ fontSize: 11, color: "var(--muted)" }}>{msg}</span>}
        <button onClick={save} style={{ ...field, cursor: "pointer", background: "var(--accent)", color: "#fff", borderColor: "var(--accent)" }}>Apply emergency stop</button>
      </div>
    </div>
  );
}
