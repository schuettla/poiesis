import { useAppStore } from "../../lib/store";
import type { TurnModifiers } from "../../lib/types";
import { EFFORTS } from "../../lib/effort";
import { shortDate } from "../../lib/lastTime";
import "./ModifierChips.css";

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

  const chips: { key: keyof TurnModifiers; label: string; fromDefault?: boolean; once?: boolean }[] = [];
  // `DEF-5`: a chip my default put there says so, and removing it is "not this
  // time": it comes back on the next message and the setting is untouched.
  if (mods.planFirst) {
    chips.push({
      key: "planFirst",
      label: mods.planFirstIsDefault ? "Plan first (default)" : "Plan first",
      fromDefault: !!mods.planFirstIsDefault,
    });
  }
  if (mods.effort) chips.push({ key: "effort", label: `Effort: ${EFFORTS.find((e) => e.value === mods.effort)?.label ?? mods.effort}` });
  if (mods.maxSteps) chips.push({ key: "maxSteps", label: `Up to ${mods.maxSteps} steps` });
  // `SEE-3`: what a composer hint was taken up on. Only ever for this message, so no default.
  if (mods.web) chips.push({ key: "web", label: "Web search on", once: true });
  if (mods.noHandover) chips.push({ key: "noHandover", label: "Do it yourself", once: true });
  if (mods.asAddition) chips.push({ key: "asAddition", label: "As an addition", once: true });
  if (mods.lastTime) {
    chips.push({ key: "lastTime", label: `Same way as ${shortDate(mods.lastTime.at)}`, once: true });
  }
  if (!chips.length) return null;

  return (
    <div className="modifier-chips" role="group" aria-label="For this message only">
      {chips.map((c) => (
        <span className="modifier-chip" key={c.key}>
          <span className="modifier-chip-glyph" aria-hidden="true">◇</span>
          <span className="modifier-chip-label">{c.label}</span>
          {!c.fromDefault && !c.once && (
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
