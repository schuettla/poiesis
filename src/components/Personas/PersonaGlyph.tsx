import type { ReactNode } from "react";
import { lookOf } from "../../lib/personaLook";
import { CellIcon, TeamIcon } from "../Icons/Icons";
import "./PersonaGlyph.css";

/** Poiesis as a leader: a cell with its nucleus, ink-coloured because it has no hue.
 * Not the live mark, which reads a run; this is only a name. */
export function PoiesisGlyph({ size = 15 }: { size?: number }) {
  return <CellIcon className="poiesis-glyph" size={size} />;
}

/**
 * `TEAM-2`: a persona's face, a letter in a circle. With `badge`, a small mark in the
 * lower right corner: on the composer's button that is the way of working's glyph, so
 * one 17px button still says both who leads and how.
 */
export default function PersonaGlyph({
  name,
  look,
  size = 15,
  badge,
}: {
  name: string;
  /** What the persona stores (`personas.look`); without it the face is made from the name. */
  look?: string | null;
  size?: number;
  badge?: ReactNode;
}) {
  const { letter, hue } = lookOf({ name, look });
  return (
    <span className="persona-glyph" data-hue={hue} style={{ width: size, height: size, fontSize: Math.round(size * 0.62) }} aria-hidden="true">
      {letter}
      {badge && <span className="persona-glyph-badge">{badge}</span>}
    </span>
  );
}

/**
 * A team as a leader (Part II.2): the team mark in its coordinator's hue, with the same badge
 * corner as a persona's face. Poiesis coordinating gives it no hue.
 */
export function TeamGlyph({ coordinator, size = 15, badge }: { coordinator?: { name: string; look?: string | null } | null; size?: number; badge?: ReactNode }) {
  const hue = coordinator ? lookOf(coordinator).hue : undefined;
  return (
    <span className="team-glyph" data-hue={hue} style={{ width: size, height: size }} aria-hidden="true">
      <TeamIcon size={size} />
      {badge && <span className="persona-glyph-badge">{badge}</span>}
    </span>
  );
}
