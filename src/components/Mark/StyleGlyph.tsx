import "./Mark.css";

/**
 * `WRK-3`/`WRK-12`/`WRK-14`: how a way of working is drawn. The same family as
 * `PoiesisMark`: a membrane, a nucleus, a dot or two. One glyph per organization
 * shape, so a person learns to read them without a label, and the label is always
 * beside them anyway.
 *
 * - Ad hoc: a plain cell. No structure.
 * - Command & Control: a nucleus with spokes to one point at the top. Everything
 *   runs through one centre, which is you.
 * - Lean: a membrane with one dot on it, one piece in flow.
 * - Mission Command: a nucleus with an arrow to a point on the membrane. The goal
 *   is set, the path is free.
 * - Swarm: several small nuclei inside one membrane. Many, no boss.
 *
 * A custom preset or an offspring is drawn with its parent's glyph (`glyph` is the
 * parent's slug), and an unknown one as the plain cell.
 *
 * Nothing here moves. A glyph is a name, not a state: what a run is doing is the
 * mark's job, and it reads real events (`ORG_LOOP_PLAN` IX.6).
 */
export default function StyleGlyph({ glyph, size = 14, branch = false }: { glyph: string; size?: number; branch?: boolean }) {
  return (
    <svg className={`style-glyph glyph-${glyph}`} width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <circle className="glyph-membrane" cx="12" cy="12" r="8" fill="none" strokeWidth="1.4" />
      {branch && (
        // `WRK-20`: an offspring is its parent's glyph with a small cell budding off it.
        <>
          <path className="glyph-line" d="M17.5 17.5 L20 20" fill="none" strokeWidth="1.2" />
          <circle className="glyph-dot" cx="21" cy="21" r="1.7" />
        </>
      )}
      {glyph === "lean" && (
        <>
          <circle className="glyph-nucleus" cx="12" cy="12" r="2.2" />
          <circle className="glyph-dot" cx="12" cy="4" r="2" />
        </>
      )}
      {glyph === "command-and-control" && (
        <>
          <circle className="glyph-nucleus" cx="12" cy="14.5" r="2.2" />
          <circle className="glyph-dot" cx="12" cy="5.5" r="1.7" />
          <path className="glyph-line" d="M12 5.5 L6.8 13 M12 5.5 L12 12.3 M12 5.5 L17.2 13" fill="none" strokeWidth="1.1" />
        </>
      )}
      {glyph === "mission-command" && (
        <>
          <circle className="glyph-nucleus" cx="9" cy="14.5" r="2.2" />
          <path className="glyph-line" d="M10.8 12.6 L15.9 7.9 M13.6 7.6 L16.2 7.6 L16.2 10.2" fill="none" strokeWidth="1.2" />
          <circle className="glyph-dot" cx="17.7" cy="6.3" r="1.8" />
        </>
      )}
      {glyph === "swarm" && (
        <>
          <circle className="glyph-nucleus" cx="9" cy="10" r="1.7" />
          <circle className="glyph-nucleus" cx="15.2" cy="10.8" r="1.7" />
          <circle className="glyph-nucleus" cx="11.8" cy="15.4" r="1.7" />
        </>
      )}
    </svg>
  );
}

/**
 * `WRK-20`: a way of working that evolution leaves alone. A small padlock after the
 * name in the `/style` menu, and nowhere on the chip: a lock is not a state of the chat.
 */
export function LockMark({ size = 11 }: { size?: number }) {
  return (
    <svg className="lock-mark" width={size} height={size} viewBox="0 0 12 12" role="img" aria-label="Locked">
      <title>Locked: I leave it as it is</title>
      <rect x="2" y="5.2" width="8" height="5.6" rx="1.2" fill="currentColor" />
      <path d="M4 5.2 V3.8 a2 2 0 0 1 4 0 V5.2" fill="none" stroke="currentColor" strokeWidth="1.2" />
    </svg>
  );
}
