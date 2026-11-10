import * as api from "../../lib/api";
import { orgWords, type OrgFacts } from "../../lib/orgState";
import { useAppStore } from "../../lib/store";
import { useRecent } from "../../lib/useRecent";
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
  tending: "tidying my head",
  listening: "waiting for your answer",
};

/** `WRK-8`: where a helper's bud sits, outside the membrane. */
const BUDS = [
  { cx: 20.9, cy: 6.9 },
  { cx: 12, cy: 22.3 },
  { cx: 3.1, cy: 6.9 },
];

/** `WRK-8`: what the active chat's organization is doing, as facts. Primitives
 * only, so a change that is not about the mark does not redraw it. */
function useOrg(): OrgFacts & { slug: string | null; pulsedAt?: number; freshAt?: number; doneAt?: number } {
  const convId = useAppStore((s) => s.activeConversationId);
  const summary = useAppStore((s) => (convId ? s.runSummaries[convId] : undefined));
  const helpers = useAppStore((s) =>
    convId ? Object.values(s.subRuns).filter((r) => r.parentConversationId === convId && api.stillWorking(r.status)).length : 0
  );
  const style = summary?.style && summary.style.preset !== "ad-hoc" ? summary.style : null;
  return {
    preset: style?.name ?? null,
    slug: style?.preset ?? null,
    live: !!summary?.live,
    helpers,
    looking: !!summary?.looking,
    branching: !!summary?.branching,
    stopped: !!summary?.stopped,
    pulsedAt: summary?.pulsedAt,
    freshAt: summary?.freshAt,
    doneAt: summary && !summary.live && summary.stopReason === "completed" ? summary.endedAt : undefined,
  };
}

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
  // `WRK-21`: a fine layer for each time I changed how I work on my own. It forms
  // while I rest, so it never competes with what a run is doing.
  const layers = useAppStore((s) => Math.min(3, s.orgGrowth?.layers ?? 0));

  // `WRK-8`: the organization, from real events and nothing else.
  const org = useOrg();
  const pulse = useRecent(org.pulsedAt, 1400);
  const fresh = useRecent(org.freshAt, 1800);
  const settled = useRecent(org.doneAt, 3800);
  const buds = Math.min(org.helpers, BUDS.length);

  const stage = growthStage(entries);
  const base = digestPending
    ? "Poiesis Agent — resting, with something to tell you"
    : `Poiesis Agent — ${LABELS[presence] ?? "resting"}`;
  const label = [base, ...orgWords(org), ...(layers > 0 && !org.live ? ["I've changed how I work on my own"] : [])].join(" · ");
  const className = [
    "poiesis-mark",
    `stage-${stage}`,
    `state-${presence}`,
    digestPending && "digest-pending",
    org.live && org.slug && `org-${org.slug}`,
    org.stopped && org.live && "org-stopped",
    pulse && "org-pulse",
    fresh && "org-fresh",
    settled && "org-done",
  ]
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
      {/* `WRK-8`: Lean's glyph shows as one dot going round the membrane while
          the run works that way. Only the shapes that exist are drawn. */}
      {org.live && org.slug === "lean" && (
        <g className="mark-flow">
          <circle cx="12" cy="4" r="1.3" />
        </g>
      )}
      {/* `WRK-12`/`WRK-14`: the other ways of working, drawn as their glyphs are, and as still:
          what moves on the mark is what happens, not what the preset is. */}
      {org.live && org.slug === "command-and-control" && (
        <g className="mark-hub">
          <circle cx="12" cy="4.6" r="1.2" />
          <path d="M12 5.8 L12 9.2 M12 5.8 L7.4 10.4 M12 5.8 L16.6 10.4" fill="none" />
        </g>
      )}
      {org.live && org.slug === "mission-command" && (
        <g className="mark-aim">
          <path d="M14 10 L16.9 7.1" fill="none" />
          <circle cx="17.66" cy="6.34" r="1.3" />
        </g>
      )}
      {org.live && org.slug === "swarm" && (
        <g className="mark-swarm">
          <circle cx="12" cy="6.4" r="1.1" />
          <circle cx="16.8" cy="14.6" r="1.1" />
          <circle cx="7.2" cy="14.6" r="1.1" />
        </g>
      )}
      {!org.live &&
        Array.from({ length: layers }, (_, i) => (
          <circle key={i} className="mark-layer" cx="12" cy="12" r={9.1 + i * 0.7} fill="none" />
        ))}
      {/* A second ring forms around the membrane while someone else looks. */}
      {org.looking && org.live && <circle className="mark-second" cx="12" cy="12" r="10.4" fill="none" />}
      {/* A helper is a small cell budded off the membrane, and gone when it is done. */}
      {org.live &&
        BUDS.slice(0, buds).map((b, i) => <circle key={i} className="mark-bud" cx={b.cx} cy={b.cy} r="1.2" fill="none" />)}
      <circle className="mark-nucleus" cx="12" cy="12" r="3" />
    </svg>
  );
}
