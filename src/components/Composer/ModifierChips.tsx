import { useAppStore } from "../../lib/store";
import type { TurnModifiers } from "../../lib/types";
import "./ModifierChips.css";

const EFFORT_WORD: Record<string, string> = {
  off: "no thinking",
  low: "low",
  medium: "medium",
  high: "high",
  provider: "model's default",
};

/**
 * `CMP-6`: what the next message alone has been told to do. One chip each,
 * above the input, gone the moment the message is sent.
 *
 * `make default` (`DEF-2`) is the other way into a setting: the same decision,
 * kept, so Settings is only where a default is *held* and the chip is where it
 * is normally *made*.
 */
export default function ModifierChips() {
  const mods = useAppStore((s) => s.turnModifiers);
  const clear = useAppStore((s) => s.clearTurnModifier);
  const makeDefault = useAppStore((s) => s.makeModifierDefault);

  const chips: { key: keyof TurnModifiers; label: string; fromDefault?: boolean }[] = [];
  // `DEF-5`: a chip my default put there says so, and removing it is "not this
  // time": it comes back on the next message and the setting is untouched.
  if (mods.planFirst) {
    chips.push({
      key: "planFirst",
      label: mods.planFirstIsDefault ? "Plan first (default)" : "Plan first",
      fromDefault: !!mods.planFirstIsDefault,
    });
  }
  if (mods.effort) chips.push({ key: "effort", label: `Think: ${EFFORT_WORD[mods.effort] ?? mods.effort}` });
  if (mods.maxSteps) chips.push({ key: "maxSteps", label: `Up to ${mods.maxSteps} steps` });
  if (!chips.length) return null;

  return (
    <div className="modifier-chips" role="group" aria-label="For this message only">
      {chips.map((c) => (
        <span className="modifier-chip" key={c.key}>
          <span className="modifier-chip-glyph" aria-hidden="true">◇</span>
          <span className="modifier-chip-label">{c.label}</span>
          {!c.fromDefault && (
            <button
              className="modifier-chip-default"
              title="Keep this as my default from now on"
              onClick={() => void makeDefault(c.key)}
            >
              make default
            </button>
          )}
          <button
            className="modifier-chip-x"
            aria-label={`Remove: ${c.label}`}
            onClick={() => clear(c.key)}
          >
            ×
          </button>
        </span>
      ))}
    </div>
  );
}
