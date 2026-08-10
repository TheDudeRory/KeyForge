import { useEffect, useState, type CSSProperties } from "react";
import { hotkeysList, hotkeysSave, type HotkeyBinding, type HotkeyAction } from "../lib/ipc";
import { type MacroInfo } from "./MacroEditor";
import { KeyField } from "./KeyPicker";
import { useUi } from "../stores/settings";
import "./macros.css";

/* Global Hotkeys tab. The list is edited locally and saved WHOLE (hotkeys_save
   takes the entire binding set and answers with the re-registered view plus the
   per-combo error map) — simpler and race-free versus index-based add/update
   /delete commands. `macros` comes from the App-level library so a RunMacro
   binding can be named and picked without a second load. */

const label: CSSProperties = { fontSize: 11, color: "var(--muted)", marginBottom: 3, display: "block" };
const field: CSSProperties = { width: "100%", boxSizing: "border-box", background: "var(--panel-2)", color: "var(--text)", border: "1px solid var(--border)", borderRadius: 4, padding: "5px 7px", fontSize: 12.5, marginBottom: 10 };

const normalize = (s: string) => s.replace(/\s+/g, "").toLowerCase();

const summarize = (a: HotkeyAction, macroName?: (id: string) => string): string => {
  switch (a.type) {
    case "launch_program": return `Launch ${a.path}${a.args && a.args.length ? " " + a.args.join(" ") : ""}`;
    case "run_command": return `Run: ${a.command}`;
    case "run_macro": return `Macro: ${macroName ? macroName(a.id) : a.id || "(none)"}`;
    default: return "";
  }
};

export default function HotkeyManager({ macros, reloadKey = 0 }: { macros: MacroInfo[]; reloadKey?: number }) {
  const confirmOnDelete = useUi((s) => s.settings.confirmOnDelete);
  const macroName = (id: string) => macros.find((m) => m.id === id)?.name ?? id ?? "(none)";

  const [bindings, setBindings] = useState<HotkeyBinding[]>([]);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [estop, setEstop] = useState("");
  const [selIdx, setSelIdx] = useState<number | null>(null);

  useEffect(() => {
    hotkeysList()
      .then((v) => {
        setBindings(v.bindings);
        setErrors(v.errors);
        setEstop(v.emergency_stop);
        setSelIdx((i) => (i != null && i < v.bindings.length ? i : v.bindings.length ? 0 : null));
      })
      .catch(() => { setBindings([]); setErrors({}); setSelIdx(null); });
  }, [reloadKey]);

  const commit = (next: HotkeyBinding[]) => {
    setBindings(next);
    hotkeysSave(next).then((v) => { setBindings(v.bindings); setErrors(v.errors); setEstop(v.emergency_stop); }).catch(() => {});
  };
  const patch = (i: number, b: Partial<HotkeyBinding>) => commit(bindings.map((x, j) => (j === i ? { ...x, ...b } : x)));
  const setAction = (i: number, action: HotkeyAction) => patch(i, { action });
  const add = () => {
    const next = [...bindings, { hotkey: "", enabled: true, action: { type: "launch_program", path: "", args: [] } as HotkeyAction }];
    commit(next);
    setSelIdx(next.length - 1);
  };
  const remove = (i: number) => {
    if (confirmOnDelete && !confirm(`Delete hotkey "${bindings[i]?.hotkey || "(unset)"}"?`)) return;
    commit(bindings.filter((_, j) => j !== i));
    setSelIdx(null);
  };

  const sel = selIdx != null ? bindings[selIdx] : undefined;
  const selAction = sel?.action;
  const selErr = sel ? errors[normalize(sel.hotkey)] : undefined;

  return (
    <>
      {/* list — left */}
      <div style={{ width: 220, flex: "0 0 220px", borderRight: "1px solid var(--border)", display: "flex", flexDirection: "column" }}>
        <div style={{ padding: "10px 12px", fontSize: 11, fontWeight: 700, letterSpacing: 0.6, color: "var(--muted)", borderBottom: "1px solid var(--border)" }}>GLOBAL HOTKEYS</div>
        <div style={{ flex: 1, overflowY: "auto", padding: 4 }}>
          {bindings.length === 0 && <div style={{ padding: 10, fontSize: 12, color: "var(--muted)" }}>No hotkeys yet.</div>}
          {bindings.map((b, i) => {
            const err = errors[normalize(b.hotkey)];
            return (
              <div key={i} onClick={() => setSelIdx(i)}
                style={{ display: "flex", alignItems: "center", gap: 7, padding: "6px 8px", borderRadius: 5, cursor: "pointer", background: i === selIdx ? "#2b3a55" : "transparent", fontSize: 12, color: i === selIdx ? "#fff" : "var(--text)", opacity: b.enabled ? 1 : 0.5 }}>
                <span title={err ?? (b.enabled ? "active" : "disabled")}
                  style={{ width: 8, height: 8, borderRadius: "50%", flex: "0 0 auto", background: err ? "#d66" : b.enabled ? "#5a5" : "#666" }} />
                <div style={{ flex: 1, overflow: "hidden" }}>
                  <div style={{ fontFamily: "monospace", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{b.hotkey || "(unset)"}</div>
                  <div style={{ fontSize: 10.5, color: "var(--muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{summarize(b.action, macroName)}</div>
                </div>
              </div>
            );
          })}
        </div>
        <button onClick={add} style={{ margin: 8, padding: "6px", background: "var(--border)", color: "var(--text)", border: "1px solid var(--border)", borderRadius: 5, cursor: "pointer", fontSize: 12 }}>＋ Add hotkey</button>
      </div>

      {/* editor — right */}
      <div style={{ flex: 1, padding: 16, overflowY: "auto" }}>
        {!sel || selIdx == null || !selAction ? (
          <div style={{ color: "var(--muted)", fontSize: 13 }}>No hotkey selected. Add one to get started.</div>
        ) : (
          <>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 12 }}>
              <span style={{ fontSize: 14, fontWeight: 600, color: "var(--text)" }}>Edit hotkey</span>
              <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <label style={{ display: "flex", alignItems: "center", gap: 5, fontSize: 12, color: "var(--text)", cursor: "pointer" }}>
                  <input type="checkbox" checked={sel.enabled} onChange={(e) => patch(selIdx, { enabled: e.target.checked })} /> Enabled
                </label>
                <button onClick={() => remove(selIdx)} style={{ fontSize: 11, padding: "4px 8px", background: "#5a2a2a", color: "#eaa", border: "none", borderRadius: 4, cursor: "pointer" }}>Delete</button>
              </div>
            </div>

            <label style={label}>Hotkey</label>
            <KeyField value={sel.hotkey} placeholder="click to set" onChange={(combo) => patch(selIdx, { hotkey: combo })} />
            {selErr && <div style={{ fontSize: 11, color: "#e88", margin: "6px 0 10px" }}>⚠ {selErr}</div>}
            <div style={{ height: selErr ? 0 : 10 }} />

            <label style={label}>Action</label>
            <select style={field} value={selAction.type}
              onChange={(e) => {
                const t = e.target.value;
                setAction(selIdx, t === "launch_program" ? { type: "launch_program", path: "", args: [] }
                  : t === "run_command" ? { type: "run_command", command: "" }
                  : { type: "run_macro", id: macros[0]?.id ?? "" });
              }}>
              <option value="launch_program">Launch program</option>
              <option value="run_command">Run command</option>
              <option value="run_macro">Run macro</option>
            </select>

            {selAction.type === "launch_program" ? (
              <>
                <label style={label}>Program path</label>
                <input style={field} placeholder="e.g. notepad.exe or C:\\path\\to\\app.exe" value={selAction.path}
                  onChange={(e) => setAction(selIdx, { type: "launch_program", path: e.target.value, args: selAction.args ?? [] })} />
                <label style={label}>Arguments (one per line)</label>
                <textarea style={{ ...field, height: 60, fontFamily: "monospace", resize: "vertical" }} value={(selAction.args ?? []).join("\n")}
                  onChange={(e) => setAction(selIdx, { type: "launch_program", path: selAction.path, args: e.target.value.split("\n").map((l) => l.trim()).filter(Boolean) })} />
              </>
            ) : selAction.type === "run_command" ? (
              <>
                <label style={label}>Command line</label>
                <input style={field} placeholder="e.g. code .   (runs through the OS shell)" value={selAction.command}
                  onChange={(e) => setAction(selIdx, { type: "run_command", command: e.target.value })} />
              </>
            ) : (
              <>
                <label style={label}>Macro</label>
                <select style={field} value={selAction.id} onChange={(e) => setAction(selIdx, { type: "run_macro", id: e.target.value })}>
                  <option value="">— select macro —</option>
                  {macros.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
                </select>
                {macros.length === 0 && <div style={{ fontSize: 11, color: "var(--muted)" }}>No macros yet — create one in the Macros tab.</div>}
              </>
            )}

            <div style={{ marginTop: 18, paddingTop: 10, borderTop: "1px solid var(--border)", fontSize: 11, color: "var(--muted)" }}>
              Emergency stop: <code style={{ fontFamily: "monospace", color: "var(--text)" }}>{estop || "—"}</code> — cancels every running
              macro and releases held keys. Rebind it in Settings → Keybinds.
            </div>
          </>
        )}
      </div>
    </>
  );
}
