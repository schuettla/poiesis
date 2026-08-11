import { useAppStore } from "../../lib/store";
import "./Mark.css";

/** Entries needed to reach each growth stage. Deliberately steep: a mark that
 * fills up in an afternoon would say nothing. Growth is earned and slow. */
const STAGES = [5, 20, 50];

/** Orbit-dot positions on the membrane, at 0° / 120° / 240°. */
const ORBITS = [
  { cx: 12, cy: 4 },
  { cx: 18.93, cy: 16 },
  { cx: 5.07, cy: 16 },
];

const LABELS: Record<string, string> = {
  idle: "resting",
  active: "working",
  reflecting: "reflecting on a past conversation",
  healing: "recovering",
};

/** How many durable entries exist, from the always-injected index. Counting the
 * index lines avoids a second source of truth (and a second round-trip). */
export function growthStage(entries: number): number {
  return STAGES.filter((threshold) => entries >= threshold).length;
}

/**
 * The living mark (PRES-1): a nucleus inside a membrane, with orbit dots it
 * earns as the durable self grows, breathing while it works.
 *
 * It is one small piece of quiet biology, not a status light — no colors, no
 * counts, nothing that needs reading. Under reduced motion nothing moves and
 * the state is carried by the label alone.
 */
export default function PoiesisMark({ size = 20 }: { size?: number }) {
  const presence = useAppStore((s) => s.presence);
  const entries = useAppStore((s) => {
    const v = s.vitality;
    return v ? v.facts + v.lessons + v.skills : 0;
  });
  // SCH-UI-2: at most one unread digest, carried as the slow pulse — no dot,
  // no count, no badge. Only shown while otherwise idle, so it never fights
  // the working/reflecting/recovering states, which already say more.
  const digestUnread = useAppStore((s) => s.digest?.unread ?? false);
  const digestPending = digestUnread && presence === "idle";

  const stage = growthStage(entries);
  const label = digestPending
    ? "Poiesis Agent — resting, with something to tell you"
    : `Poiesis Agent — ${LABELS[presence] ?? "resting"}`;
  const className = ["poiesis-mark", `stage-${stage}`, `state-${presence}`, digestPending && "digest-pending"]
    .filter(Boolean)
    .join(" ");

  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      role="img"
      aria-label={label}
    >
      <title>{label}</title>
      {/* Not yet grown: the membrane is still forming. */}
      <circle
        className="mark-membrane"
        cx="12"
        cy="12"
        r="8"
        fill="none"
        strokeWidth="1"
        strokeDasharray={stage === 0 ? "2 3" : undefined}
      />
      <g className="mark-orbits">
        {ORBITS.slice(0, stage).map((o, i) => (
          <circle key={i} className="mark-orbit" cx={o.cx} cy={o.cy} r="1.5" />
        ))}
      </g>
      <circle className="mark-nucleus" cx="12" cy="12" r="3" />
    </svg>
  );
}
