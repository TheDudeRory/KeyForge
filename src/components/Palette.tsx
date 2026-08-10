import { useEffect, useState, type CSSProperties } from "react";
import { fuzzyFilter } from "../lib/fuzzy";
import { useBackdropClose } from "../lib/useBackdropClose";
import { useUi } from "../stores/settings";
import { macrosRun, macrosStopAll, type MacroInfo } from "./MacroEditor";

// Fuzzy command palette over everything the window can do plus every macro in
// the library — running a macro from here is the same dispatcher call the global
// hotkeys use, so a macro is reachable without binding a combo to it first.

interface Cmd {
  id: string;
  label: string;
  run: () => void;
}

function buildCommands(macros: MacroInfo[], onNewMacro: () => void, onReload: () => void): Cmd[] {
  const s = useUi.getState();
  const cmds: Omit<Cmd, "id">[] = [];
  macros.forEach((m) => cmds.push({ label: `Run macro: ${m.name}`, run: () => { macrosRun(m.id).catch(() => {}); } }));
  cmds.push(
    { label: "Stop all running macros", run: () => { macrosStopAll().catch(() => {}); } },
    { label: "New macro…", run: onNewMacro },
    { label: "Reload hotkeys + macros", run: onReload },
    { label: "Go to Hotkeys tab", run: () => s.setTab("hotkeys") },
    { label: "Go to Macros tab", run: () => s.setTab("macros") },
    { label: "Go to Devices tab", run: () => s.setTab("devices") },
    { label: "Settings…", run: () => s.openSettings(true) },
    { label: "Keybinds settings…", run: () => s.openSettings(true) },
  );
  return cmds.map((c, i) => ({ ...c, id: String(i) }));
}

const overlay: CSSProperties = { position: "fixed", inset: 0, background: "rgba(0,0,0,0.4)", display: "flex", justifyContent: "center", alignItems: "flex-start", paddingTop: "12vh", zIndex: 120 };

export default function Palette({ macros, onNewMacro, onReload }: { macros: MacroInfo[]; onNewMacro: () => void; onReload: () => void }) {
  const open = useUi((s) => s.paletteOpen);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);

  useEffect(() => {
    if (open) { setQuery(""); setSel(0); }
  }, [open]);

  const backdrop = useBackdropClose(() => useUi.getState().openPalette(false));
  if (!open) return null;
  const close = () => useUi.getState().openPalette(false);
  const results = fuzzyFilter(query, buildCommands(macros, onNewMacro, onReload), (c) => c.label).slice(0, 40);
  const clampedSel = Math.min(sel, Math.max(0, results.length - 1));
  const run = (c?: Cmd) => { if (!c) return; close(); c.run(); };

  return (
    <div style={overlay} {...backdrop}>
      <div onClick={(e) => e.stopPropagation()} style={{ width: 520, maxWidth: "90vw", background: "var(--panel)", border: "1px solid var(--border)", borderRadius: 8, boxShadow: "0 12px 48px rgba(0,0,0,0.6)", overflow: "hidden" }}>
        <input
          autoFocus
          placeholder="Type a command…"
          value={query}
          onChange={(e) => { setQuery(e.target.value); setSel(0); }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") { e.preventDefault(); setSel((v) => Math.min(v + 1, results.length - 1)); }
            else if (e.key === "ArrowUp") { e.preventDefault(); setSel((v) => Math.max(v - 1, 0)); }
            else if (e.key === "Enter") { e.preventDefault(); run(results[clampedSel]); }
            else if (e.key === "Escape") { e.preventDefault(); close(); }
          }}
          style={{ width: "100%", boxSizing: "border-box", padding: "12px 14px", fontSize: 14, background: "var(--panel)", color: "var(--text)", border: "none", borderBottom: "1px solid var(--border)", outline: "none" }}
        />
        <div style={{ maxHeight: 360, overflowY: "auto" }}>
          {results.length === 0 && <div style={{ padding: 14, color: "var(--muted)", fontSize: 13 }}>No matching commands</div>}
          {results.map((c, i) => (
            <div
              key={c.id}
              onMouseEnter={() => setSel(i)}
              onClick={() => run(c)}
              style={{ padding: "8px 14px", fontSize: 13, cursor: "pointer", color: i === clampedSel ? "#fff" : "var(--muted)", background: i === clampedSel ? "#2b3a55" : "transparent" }}
            >
              {c.label}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
