import { type ComponentType, useEffect, useState } from "react";
import { appVersion, statePath } from "../lib/ipc";
import { useUi } from "../stores/settings";
import type { TabId, Theme } from "../lib/persist";
import { useBackdropClose } from "../lib/useBackdropClose";
import { ACCENT_SELECTED, field, label, overlay, row } from "../lib/uiStyles";
import KeybindsSettings from "./KeybindsSettings";

// ── Category registry ─────────────────────────────────────────────────────────
// Left-list / right-editor split. A new category is one entry here plus one
// component — no other edits to this file.
export interface SettingsCategory {
  id: string;
  label: string;
  Component: ComponentType;
}

export const SETTINGS_CATEGORIES: SettingsCategory[] = [
  { id: "general", label: "General", Component: GeneralCategory },
  { id: "keybinds", label: "Keybinds", Component: KeybindsSettings },
];

export default function SettingsModal() {
  const open = useUi((s) => s.settingsOpen);
  const close = useUi((s) => s.openSettings);
  const [catId, setCatId] = useState<string>(SETTINGS_CATEGORIES[0].id);
  const backdrop = useBackdropClose(() => close(false));

  if (!open) return null;
  const cat = SETTINGS_CATEGORIES.find((c) => c.id === catId) ?? SETTINGS_CATEGORIES[0];
  const Body = cat.Component;

  return (
    <div style={overlay} {...backdrop}>
      <div onClick={(e) => e.stopPropagation()} style={{ width: 640, maxWidth: "92vw", height: 460, maxHeight: "88vh", display: "flex", background: "var(--panel)", border: "1px solid var(--border)", borderRadius: 10, overflow: "hidden", boxShadow: "0 12px 48px rgba(0,0,0,0.6)" }}>
        {/* category list */}
        <div style={{ width: 190, borderRight: "1px solid var(--border)", display: "flex", flexDirection: "column", background: "var(--panel)" }}>
          <div style={{ padding: "10px 12px", fontSize: 11, fontWeight: 700, letterSpacing: 0.6, color: "var(--muted)", borderBottom: "1px solid var(--border)" }}>SETTINGS</div>
          <div style={{ flex: 1, overflowY: "auto", padding: 4 }}>
            {SETTINGS_CATEGORIES.map((c) => (
              <div key={c.id} onClick={() => setCatId(c.id)}
                style={{ padding: "6px 8px", borderRadius: 5, cursor: "pointer", background: c.id === catId ? ACCENT_SELECTED : "transparent", fontSize: 12.5, color: c.id === catId ? "#fff" : "var(--text)" }}>
                {c.label}
              </div>
            ))}
          </div>
          <button onClick={() => close(false)} style={{ margin: 8, padding: "6px", background: "var(--accent)", color: "#fff", border: "none", borderRadius: 5, cursor: "pointer", fontSize: 12 }}>Done</button>
        </div>

        {/* editor */}
        <div style={{ flex: 1, padding: 16, overflowY: "auto" }}>
          <Body />
        </div>
      </div>
    </div>
  );
}

// ── General ───────────────────────────────────────────────────────────────────

function GeneralCategory() {
  const settings = useUi((s) => s.settings);
  const update = useUi((s) => s.updateSettings);
  const [version, setVersion] = useState("");
  const [path, setPath] = useState("");

  useEffect(() => {
    appVersion().then(setVersion).catch(() => setVersion(""));
    statePath().then(setPath).catch(() => setPath(""));
  }, []);

  return (
    <>
      <div style={row}>
        <span style={label}>Theme</span>
        <select style={field} value={settings.theme} onChange={(e) => update({ theme: e.target.value as Theme })}>
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </select>
      </div>
      <div style={row}>
        <span style={label}>Interface scale (%) <span style={{ color: "var(--muted)", fontSize: 10 }}>(zooms the whole UI)</span></span>
        <input style={{ ...field, width: 70 }} type="number" min={50} max={300} step={10} value={Math.round(settings.uiScale * 100)}
          onChange={(e) => update({ uiScale: Math.min(3, Math.max(0.5, (Number(e.target.value) || 100) / 100)) })} />
      </div>
      <div style={row}>
        <span style={label}>Open on tab</span>
        <select style={field} value={settings.startTab} onChange={(e) => update({ startTab: e.target.value as TabId })}>
          <option value="hotkeys">Hotkeys</option>
          <option value="macros">Macros</option>
          <option value="devices">Devices</option>
        </select>
      </div>
      <div style={{ ...row, borderBottom: "none" }}>
        <span style={label}>Confirm before deleting a hotkey or macro</span>
        <input type="checkbox" checked={settings.confirmOnDelete} onChange={(e) => update({ confirmOnDelete: e.target.checked })} />
      </div>

      <div style={{ marginTop: 18, paddingTop: 10, borderTop: "1px solid var(--border)", fontSize: 11, color: "var(--muted)", lineHeight: 1.7 }}>
        <div>KeyForge {version && `v${version}`}</div>
        {path && <div>Settings file: <code style={{ fontFamily: "monospace" }}>{path}</code></div>}
        <div>Hotkey profile and macro library live next to the executable, in <code style={{ fontFamily: "monospace" }}>hotkeys/</code> and <code style={{ fontFamily: "monospace" }}>macros/</code>.</div>
      </div>
    </>
  );
}
