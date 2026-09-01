// The in-app keymap: combos handled by the WEBVIEW while KeyForge has focus.
// Distinct from the two OS-level things a combo can also mean here:
//   - Global Hotkeys (hotkeys/default.json, Rust) — fire while any app is focused
//   - user global hotkeys (Rust hotkeys module) — fire while any app is focused
// This map is only the window's own commands, and both the central key handler
// (App.tsx) and the Keybinds settings category read it, so nothing drifts.

export type Keymap = Record<string, string>;

/** Ordered action -> label list for the editable in-app keymap. */
export const KEYBIND_ACTIONS: [string, string][] = [
  ["palette", "Command palette"],
  ["settings", "Settings"],
  ["hotkeysTab", "Go to Hotkeys tab"],
  ["macrosTab", "Go to Macros tab"],
  ["devicesTab", "Go to Devices tab"],
  ["newMacro", "New macro"],
  ["reload", "Reload lists"],
  ["stopAll", "Stop all running macros"],
];

export const DEFAULT_KEYMAP: Keymap = {
  palette: "Ctrl+Shift+P",
  settings: "Ctrl+,",
  hotkeysTab: "Ctrl+1",
  macrosTab: "Ctrl+2",
  devicesTab: "Ctrl+3",
  newMacro: "Ctrl+N",
  reload: "Ctrl+Shift+R",
  stopAll: "Ctrl+Shift+X",
};

/** Physical-key token for a combo, from `KeyboardEvent.code` so the map does not
 *  shift with the user's keyboard layout. Returns null for modifier-only and
 *  unsupported keys, which is what tells a capture to keep listening. */
export function comboKeyToken(e: KeyboardEvent): string | null {
  const c = e.code;
  if (c.startsWith("Key")) return c.slice(3);
  if (c.startsWith("Digit")) return c.slice(5);
  if (c.startsWith("Arrow")) return c.slice(5);
  if (c.startsWith("Numpad")) {
    const n = c.slice(6);
    if (/^\d$/.test(n)) return n;
    if (n === "Add") return "=";
    if (n === "Subtract") return "-";
    return null;
  }
  return ({ Comma: ",", Slash: "/", Equal: "=", Minus: "-" } as Record<string, string>)[c] ?? null;
}

export function eventToCombo(e: KeyboardEvent): string | null {
  const key = comboKeyToken(e);
  if (!key) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

/** Canonical modifier order + spelling, so "Cmd+Shift+P" and "Shift+Ctrl+P"
 *  compare equal to what eventToCombo produces. */
export function normalizeCombo(combo: string): string {
  const parts = combo.split("+");
  const key = parts.pop() ?? "";
  const mods = new Set(parts.map((m) => m.trim().toLowerCase()));
  const out: string[] = [];
  if (mods.has("ctrl") || mods.has("control") || mods.has("cmd") || mods.has("cmdorctrl") || mods.has("command")) out.push("Ctrl");
  if (mods.has("alt") || mods.has("option")) out.push("Alt");
  if (mods.has("shift")) out.push("Shift");
  out.push(key);
  return out.join("+");
}

/** The action bound to this event, or null. First match wins — the same rule the
 *  Keybinds category's conflict badge warns about. */
export function actionFor(keymap: Keymap, e: KeyboardEvent): string | null {
  const combo = eventToCombo(e);
  if (!combo) return null;
  const want = normalizeCombo(combo);
  for (const [action] of KEYBIND_ACTIONS) {
    const bound = keymap[action];
    if (bound && normalizeCombo(bound) === want) return action;
  }
  return null;
}
