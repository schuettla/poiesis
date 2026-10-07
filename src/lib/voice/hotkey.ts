// The voice hotkey (VOC-UI-9): a setting such as `Ctrl+Shift+Space`, matched
// against a key press. Only works while Poiesis has the focus.

export const DEFAULT_VOICE_HOTKEY = "Ctrl+Shift+Space";

// The key in use. App sets it at launch; the Voice tab changes it, so a new key
// works at once without a restart.
let current = DEFAULT_VOICE_HOTKEY;
export const getVoiceHotkey = (): string => current;
export const setVoiceHotkey = (spec: string): void => {
  current = spec || DEFAULT_VOICE_HOTKEY;
};

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "AltGraph"]);
// Keys that already mean something everywhere in the app.
const RESERVED_KEYS = new Set(["Escape", "Tab", "Enter", "Backspace", "Delete"]);

/** The setting text for a key press while the user picks a new hotkey, or null
 * while the press is not a usable combination yet (only a modifier, no
 * Ctrl/Alt/Cmd, or a reserved key). Shift alone is not enough: it would fire
 * while typing. */
export function hotkeyFromEvent(
  e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
): string | null {
  if (MODIFIER_KEYS.has(e.key) || RESERVED_KEYS.has(e.key)) return null;
  if (!(e.ctrlKey || e.metaKey || e.altKey)) return null;
  const key = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  return [e.ctrlKey || e.metaKey ? "Ctrl" : "", e.altKey ? "Alt" : "", e.shiftKey ? "Shift" : "", key]
    .filter(Boolean)
    .join("+");
}

const NAMES: Record<string, string> = { space: " ", esc: "Escape", escape: "Escape", enter: "Enter" };

/** True when the key press is exactly the combination in `spec`. */
export function matchesHotkey(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">, spec: string): boolean {
  const parts = spec
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean);
  const key = parts.pop();
  if (!key) return false;
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const wanted = NAMES[key.toLowerCase()] ?? key;
  // Ctrl and Cmd are one thing here, as in the rest of the app.
  const ctrl = mods.has("ctrl") || mods.has("cmd") || mods.has("meta");
  return (
    e.key.toLowerCase() === wanted.toLowerCase() &&
    (e.ctrlKey || e.metaKey) === ctrl &&
    e.shiftKey === mods.has("shift") &&
    e.altKey === mods.has("alt")
  );
}
