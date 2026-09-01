import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HotkeyBinding, MacroTestDone, MacroTestStep } from "./ipc";

// Every wrapper in ipc.ts is a one-liner over `invoke`/`listen`, so the only
// thing worth testing is the only thing that can be wrong: the command/event
// NAME and the argument SHAPE. Both are contracts with Rust that TypeScript
// cannot check — a typo in a string literal compiles fine and fails at runtime
// in front of a user. The names asserted here are the ones registered in
// src-tauri/src/lib.rs's `invoke_handler`.
const calls: Array<{ cmd: string; args: unknown }> = [];
const listens: Array<{ event: string }> = [];
let listenPayload: (e: { payload: unknown }) => void = () => undefined;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => {
    calls.push({ cmd, args });
    return Promise.resolve(null);
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    listens.push({ event });
    listenPayload = cb;
    return Promise.resolve(() => undefined);
  },
}));

const ipc = await import("./ipc");

beforeEach(() => {
  calls.length = 0;
  listens.length = 0;
});

describe("hotkey ipc wrappers", () => {
  it("uses the command names src-tauri/src/hotkeys/mod.rs registers", async () => {
    await ipc.hotkeysList();
    await ipc.hotkeysSave([]);
    await ipc.hotkeysSetEstop("Ctrl+Alt+End");
    expect(calls.map((c) => c.cmd)).toEqual(["hotkeys_list", "hotkeys_save", "hotkeys_set_estop"]);
  });

  it("passes bindings and the estop combo under the keys Rust names", async () => {
    const bindings: HotkeyBinding[] = [
      { hotkey: "Ctrl+Alt+K", enabled: true, action: { type: "run_macro", id: "m1" } },
    ];
    await ipc.hotkeysSave(bindings);
    await ipc.hotkeysSetEstop("Ctrl+Alt+End");
    expect(calls).toEqual([
      { cmd: "hotkeys_save", args: { bindings } },
      { cmd: "hotkeys_set_estop", args: { combo: "Ctrl+Alt+End" } },
    ]);
  });

  it("takes no arguments for the list command", async () => {
    await ipc.hotkeysList();
    // Passing an args object to a command that declares none is how a rename
    // silently starts being ignored.
    expect(calls[0].args).toBeUndefined();
  });
});

describe("macro test-run ipc wrappers", () => {
  it("sends the draft under `macro`, and the run id as `runId`", async () => {
    const draft = { id: "d", name: "draft", steps: [] };
    await ipc.macrosTestRun(draft);
    await ipc.macrosTestStep(draft, 2);
    await ipc.macrosTestCancel(7);
    expect(calls).toEqual([
      { cmd: "macros_test_run", args: { macro: draft } },
      { cmd: "macros_test_step", args: { macro: draft, index: 2 } },
      // Tauri v2 converts camelCase args to the snake_case Rust parameter
      // (`run_id`), so the wrapper must NOT pre-convert it.
      { cmd: "macros_test_cancel", args: { runId: 7 } },
    ]);
  });

  it("unwraps the progress payloads for the caller", async () => {
    const steps: unknown[] = [];
    await ipc.onMacroTestStep((s) => steps.push(s));
    expect(listens).toEqual([{ event: "macro-test-step" }]);
    const payload: MacroTestStep = {
      run_id: 1, index: 0, total: 3, summary: "Key down: A", status: "ok",
    };
    listenPayload({ payload });
    expect(steps).toEqual([payload]);

    const dones: unknown[] = [];
    await ipc.onMacroTestDone((d) => dones.push(d));
    expect(listens.map((l) => l.event)).toEqual(["macro-test-step", "macro-test-done"]);
    const done: MacroTestDone = { run_id: 1, status: "completed", error: null };
    listenPayload({ payload: done });
    expect(dones).toEqual([done]);
  });
});

describe("state ipc wrappers", () => {
  it("uses the portable-state command names src-tauri/src/state.rs registers", async () => {
    await ipc.loadState();
    await ipc.saveState("{}");
    await ipc.logsDir();
    await ipc.writeText("/tmp/x.txt", "body");
    expect(calls).toEqual([
      { cmd: "load_state", args: undefined },
      { cmd: "save_state", args: { json: "{}" } },
      { cmd: "logs_dir", args: undefined },
      { cmd: "write_text", args: { path: "/tmp/x.txt", content: "body" } },
    ]);
  });
});
