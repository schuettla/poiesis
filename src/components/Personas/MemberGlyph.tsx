import type { Who } from "../../lib/teamLive";
import PersonaGlyph, { PoiesisGlyph } from "./PersonaGlyph";
import "./PersonaGlyph.css";

/**
 * `TEAM-14`: a team place as a small face: a persona's letter, Poiesis's cell, or `(·)` for a
 * plain helper. The same three the chart draws, for the places the chart is too big for: the
 * Members tab and the organization strip.
 */
export default function MemberGlyph({ who, size = 15 }: { who: Who; size?: number }) {
  if (who.kind === "persona") return <PersonaGlyph name={who.name} look={who.persona?.look} size={size} />;
  if (who.kind === "poiesis") return <PoiesisGlyph size={size} />;
  return (
    <span className="member-glyph-helper" style={{ width: size, height: size, fontSize: Math.round(size * 0.9) }} aria-hidden="true">
      ·
    </span>
  );
}
