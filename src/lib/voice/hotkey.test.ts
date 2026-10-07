import { describe, expect, it } from "vitest";
import { DEFAULT_VOICE_HOTKEY, hotkeyFromEvent, matchesHotkey } from "./hotkey";

const press = (key: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "shiftKey" | "altKey", boolean>> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

describe("matchesHotkey (VOC-UI-9)", () => {
  it("matches the default combination", () => {
    expect(matchesHotkey(press(" ", { ctrlKey: true, shiftKey: true }), DEFAULT_VOICE_HOTKEY)).toBe(true);
  });

  it("needs every modifier and no extra one", () => {
    expect(matchesHotkey(press(" ", { ctrlKey: true }), DEFAULT_VOICE_HOTKEY)).toBe(false);
    expect(matchesHotkey(press(" ", { ctrlKey: true, shiftKey: true, altKey: true }), DEFAULT_VOICE_HOTKEY)).toBe(false);
    expect(matchesHotkey(press(" "), DEFAULT_VOICE_HOTKEY)).toBe(false);
  });

  it("treats Cmd as Ctrl and ignores case", () => {
    expect(matchesHotkey(press("v", { metaKey: true, altKey: true }), "ctrl+alt+V")).toBe(true);
  });

  it("does not match another key, or an empty setting", () => {
    expect(matchesHotkey(press("x", { ctrlKey: true, shiftKey: true }), DEFAULT_VOICE_HOTKEY)).toBe(false);
    expect(matchesHotkey(press("x"), "")).toBe(false);
  });
});

describe("hotkeyFromEvent (picking a new key)", () => {
  it("writes a combination the matcher accepts again", () => {
    for (const [key, mods] of [
      [" ", { ctrlKey: true, shiftKey: true }],
      ["v", { ctrlKey: true, altKey: true }],
      ["F9", { altKey: true }],
    ] as const) {
      const e = press(key, mods);
      const spec = hotkeyFromEvent(e);
      expect(spec).not.toBeNull();
      expect(matchesHotkey(e, spec!)).toBe(true);
    }
    expect(hotkeyFromEvent(press(" ", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+Space");
    expect(hotkeyFromEvent(press("v", { metaKey: true, altKey: true }))).toBe("Ctrl+Alt+V");
  });

  it("waits for a real key and refuses keys that would break typing", () => {
    expect(hotkeyFromEvent(press("Control", { ctrlKey: true }))).toBeNull();
    expect(hotkeyFromEvent(press("a"))).toBeNull();
    expect(hotkeyFromEvent(press("a", { shiftKey: true }))).toBeNull();
    expect(hotkeyFromEvent(press("Escape", { ctrlKey: true }))).toBeNull();
  });
});
