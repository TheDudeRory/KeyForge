import { describe, expect, it } from "vitest";
import { DEFAULT_KEYMAP, KEYBIND_ACTIONS, actionFor, eventToCombo, normalizeCombo } from "./keymap";

const key = (init: Partial<KeyboardEvent> & { code: string }) => init as unknown as KeyboardEvent;

describe("combo capture", () => {
  it("builds combos from the PHYSICAL key, in a fixed modifier order", () => {
    // e.key would be "P" or "p" depending on Shift; e.code never moves.
    expect(eventToCombo(key({ code: "KeyP", ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+P");
    expect(eventToCombo(key({ code: "Digit1", ctrlKey: true }))).toBe("Ctrl+1");
    expect(eventToCombo(key({ code: "Comma", ctrlKey: true }))).toBe("Ctrl+,");
  });

  it("returns null for keys a binding cannot be made of, so capture keeps listening", () => {
    expect(eventToCombo(key({ code: "ControlLeft", ctrlKey: true }))).toBeNull();
    expect(eventToCombo(key({ code: "F13" }))).toBeNull();
  });
});

describe("normalizeCombo", () => {
  it("collapses the modifier spellings that mean the same key", () => {
    expect(normalizeCombo("Cmd+Shift+P")).toBe("Ctrl+Shift+P");
    expect(normalizeCombo("CmdOrCtrl+Shift+P")).toBe("Ctrl+Shift+P");
    expect(normalizeCombo("shift+control+P")).toBe("Ctrl+Shift+P");
    expect(normalizeCombo("Option+K")).toBe("Alt+K");
  });
});

describe("actionFor", () => {
  it("matches a bound action however the stored combo is spelled", () => {
    expect(actionFor(DEFAULT_KEYMAP, key({ code: "KeyP", ctrlKey: true, shiftKey: true }))).toBe("palette");
    expect(actionFor({ ...DEFAULT_KEYMAP, palette: "cmd+shift+P" }, key({ code: "KeyP", ctrlKey: true, shiftKey: true }))).toBe("palette");
  });

  it("answers null for an unbound combo", () => {
    expect(actionFor(DEFAULT_KEYMAP, key({ code: "KeyJ", ctrlKey: true }))).toBeNull();
  });

  it("ships a default for every editable action, with no duplicate combos", () => {
    const combos = KEYBIND_ACTIONS.map(([a]) => {
      const c = DEFAULT_KEYMAP[a];
      expect(c, `no default combo for "${a}"`).toBeTruthy();
      return normalizeCombo(c);
    });
    expect(new Set(combos).size).toBe(combos.length);
  });
});
