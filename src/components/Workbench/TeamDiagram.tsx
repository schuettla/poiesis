import { Fragment, useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import type { Persona, Team } from "../../lib/api";
import { lookOf } from "../../lib/personaLook";
import { CHART_H, CHART_W, edgePath, layoutTeam, pairsOf, type Place } from "../../lib/teamChart";
import { pairKey, postWidth, type TaskState } from "../../lib/teamLive";

/**
 * `TEAM-9`: a team as the chart `TEAMS_PLAN` IV.2 describes: You on top, the coordinator under
 * You, the members in the places the shape gives them, and the lines between them. It is the
 * team half of `OrgDiagram` (the same classes, the same fixed box), so the editor in
 * Settings → Personas & teams and, in `TEAM-14`, the live Coordination view draw one picture.
 *
 * Without `edit` it only draws. With `edit` the places and the lines can be clicked: the
 * caller keeps what is picked and what to do about it.
 */

export interface TeamChartEdit {
  /** The place a line would start from: `you`, `coordinator` or a role. */
  picked: string | null;
  /** The key of the line (`a|b`) that was clicked. */
  selected: string | null;
  onPlace: (key: string) => void;
  onLine: (key: string) => void;
}

/** `TEAM-14`: what the live Coordination view adds to the drawing. Everything here comes from
 * the run's events (`lib/teamLive.ts`); without it the chart is only the team. */
export interface TeamChartLive {
  /** How many posts went along each line in this task, by `pairKey`: the line's width. */
  posts?: Record<string, number>;
  /** The plan's items by the role that has them: small dots next to that member. */
  items?: Record<string, { n: number; text: string; state: string; word: string }[]>;
  /** `TEAM-16`: the line the replay is on, by `pairKey`: it is drawn in the sender's hue. */
  current?: string;
  /** Who sent the post the replay is on: the current line takes their hue. */
  currentFrom?: string;
  /** The post that just went out (live) or that the replay just stepped to: it travels along
   * its line once, from the sender to the one it is for, and the receiver pings. Keyed by `id`,
   * so each post flies once. Nothing moves under reduced motion. */
  step?: { id: string; from: string; to: string };
  /** What the sentence under the chart is: a run going on, or a replay. Only its dot. */
  tone?: "live" | "replay";
  /** A member's place that opens its task chat when pressed, by role. */
  open?: Record<string, () => void>;
  /** A second look before the answer goes out (Mission Command): the gate on You's line. */
  gate?: boolean;
  /** The line is stopped. */
  stopped?: boolean;
  /** I am waiting for you: the quiet glow where You are. */
  waiting?: boolean;
}

export interface TeamChartProps {
  team: Pick<Team, "coordinator" | "shape" | "members" | "lines">;
  personas: Pick<Persona, "id" | "name" | "look">[];
  edit?: TeamChartEdit;
  /** What the chart says in words, for the screen reader and under the picture. */
  sentence?: string;
  /** Roles that are working right now (`TEAM-14`): drawn with the orb. */
  working?: string[];
  live?: TeamChartLive;
}

/** Click and Enter or Space, for something that is not a `<button>`. */
export function asButton(act: () => void) {
  return {
    role: "button" as const,
    tabIndex: 0,
    onClick: act,
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        act();
      }
    },
  };
}

const short = (s: string, n: number) => (s.length > n ? `${s.slice(0, Math.max(1, n - 1))}…` : s);

const MAX_DOTS = 6;

/** The smallest box round the places, their names and item dots, and the lines (a curve stays inside
 * its control points, so every number pair in a path counts), with a margin. Never smaller than
 * `MIN_W` × `MIN_H`, so two people are not drawn huge. */
const MIN_W = 300;
const MIN_H = 170;
function fitBox(places: Place[], paths: string[], extra: { x: number; y: number }[]): string {
  const xs: number[] = [];
  const ys: number[] = [];
  for (const p of places) {
    const half = Math.max(p.r + 6, Math.min(p.room, p.key.length * 6.2) / 2);
    xs.push(p.x - half, p.x + half);
    ys.push(p.y - p.r - 8, p.y + p.r + 30);
  }
  for (const d of paths) {
    const n = d.match(/-?\d+(\.\d+)?/g)?.map(Number) ?? [];
    for (let i = 0; i + 1 < n.length; i += 2) {
      xs.push(n[i]);
      ys.push(n[i + 1]);
    }
  }
  for (const e of extra) {
    xs.push(e.x);
    ys.push(e.y);
  }
  if (!xs.length) return `0 0 ${CHART_W} ${CHART_H}`;
  const pad = 12;
  let [x0, x1, y0, y1] = [Math.min(...xs) - pad, Math.max(...xs) + pad, Math.min(...ys) - pad, Math.max(...ys) + pad];
  if (x1 - x0 < MIN_W) [x0, x1] = [(x0 + x1 - MIN_W) / 2, (x0 + x1 + MIN_W) / 2];
  if (y1 - y0 < MIN_H) [y0, y1] = [(y0 + y1 - MIN_H) / 2, (y0 + y1 + MIN_H) / 2];
  const f = (v: number) => Math.round(v * 10) / 10;
  return `${f(x0)} ${f(y0)} ${f(x1 - x0)} ${f(y1 - y0)}`;
}

export default function TeamDiagram({ team, personas, edit, sentence, working = [], live }: TeamChartProps) {
  const arrowId = `org-arrow-${useId().replace(/:/g, "")}`;
  const places = layoutTeam(team);
  const at = new Map(places.map((p) => [p.key, p]));
  const pairs = pairsOf(team.lines);
  const personaOf = (ref: string) => personas.find((p) => `persona:${p.id}` === ref);
  // The gate sits beside the line between You and the coordinator, where the answer goes out.
  const [you, lead] = [at.get("you"), at.get("coordinator")];
  const gateAt = you && lead ? { x: (you.x + lead.x) / 2 + 24, y: (you.y + lead.y) / 2 } : null;

  /** What a place is, for its tooltip and its look. */
  const who = (p: Place) => {
    if (p.key === "you") return { kind: "you" as const, title: "You" };
    const ref = p.key === "coordinator" ? team.coordinator : team.members.find((m) => m.role === p.key)?.persona;
    const persona = ref ? personaOf(ref) : undefined;
    const member = team.members.find((m) => m.role === p.key);
    const role = p.key === "coordinator" ? "coordinator" : p.key;
    if (!ref || ref === "helper") {
      return { kind: "helper" as const, title: `${role}, a helper${member?.job ? `: ${member.job}` : ""}` };
    }
    if (ref === "poiesis") return { kind: "lead" as const, title: `${role}, Poiesis` };
    return {
      kind: "persona" as const,
      persona,
      title: `${role}, ${persona?.name ?? "a persona that is gone"}${member?.job ? `: ${member.job}` : ""}`,
    };
  };
  /** A place's hue, when it is a persona: what its posts travel in. */
  const hueOf = (key: string): number | undefined => {
    const p = at.get(key);
    const w = p ? who(p) : undefined;
    return w?.kind === "persona" && w.persona ? lookOf(w.persona).hue : undefined;
  };

  // The line a post travels along, in the direction it went.
  const step = live?.step;
  const flight = (() => {
    if (!step) return null;
    const from = at.get(step.from);
    const to = at.get(step.to);
    if (!from || !to || from.key === to.key) return null;
    const pair = pairs.find((p) => p.key === pairKey(step.from, step.to));
    if (!pair) return { d: edgePath(from, to, []), reverse: false, to };
    const [a, b] = [at.get(pair.a)!, at.get(pair.b)!];
    return { d: edgePath(a, b, places.filter((p) => p.key !== a.key && p.key !== b.key)), reverse: step.from !== pair.a, to };
  })();

  // The live view fits the box to the team, so a small team is drawn large; the editor keeps the
  // whole canvas, so nothing jumps while members come and go.
  const viewBox = edit || !live ? `0 0 ${CHART_W} ${CHART_H}` : fitBox(places, pairs.map((p) => {
    const a = at.get(p.a);
    const b = at.get(p.b);
    return a && b ? edgePath(a, b, places.filter((o) => o.key !== a.key && o.key !== b.key)) : "";
  }), live.gate && gateAt ? [{ x: gateAt.x + 40, y: gateAt.y }] : []);

  return (
    <section className="org-diagram team" aria-label="The team as a chart">
      <svg
        className={`org-svg team-svg${edit ? " editing" : ""}${live?.stopped ? " stopped" : ""}`}
        viewBox={viewBox}
        role="group"
        aria-label={sentence ?? "The team as a chart"}
      >
        <defs>
          {/* In the drawing's own units, so a line that grew wide with posts keeps a small head. */}
          <marker id={arrowId} viewBox="0 0 8 8" refX="7" refY="4" markerUnits="userSpaceOnUse" markerWidth="9" markerHeight="9" orient="auto-start-reverse">
            <path className="org-arrow" d="M0 0.8 L7 4 L0 7.2 Z" />
          </marker>
        </defs>
        {pairs.map((pair) => {
          const a = at.get(pair.a);
          const b = at.get(pair.b);
          if (!a || !b) return null;
          const d = edgePath(a, b, places.filter((p) => p.key !== a.key && p.key !== b.key));
          const marks = {
            markerEnd: pair.toB ? `url(#${arrowId})` : undefined,
            markerStart: pair.toA ? `url(#${arrowId})` : undefined,
          };
          const posts = live?.posts?.[pair.key] ?? 0;
          const isCurrent = live?.current === pair.key;
          return (
            <g
              key={pair.key}
              className={`org-line${edit?.selected === pair.key ? " selected" : ""}${isCurrent ? " current" : ""}${posts > 0 ? " busy" : ""}`}
              data-line={pair.key}
              data-hue={isCurrent && live?.currentFrom ? hueOf(live.currentFrom) : undefined}
            >
              {isCurrent && <path className="org-edge-glow" d={d} />}
              <path
                className={`org-edge k-${pair.kind}`}
                d={d}
                style={posts > 0 ? { strokeWidth: postWidth(posts) } : undefined}
                {...marks}
              />
              {edit && (
                <path
                  className="org-hit"
                  d={d}
                  aria-label={`${pair.a} and ${pair.b}: ${pair.lines.length} ${pair.lines.length === 1 ? "line" : "lines"}`}
                  {...asButton(() => edit.onLine(pair.key))}
                />
              )}
            </g>
          );
        })}
        {places.map((p, i) => {
          const w = who(p);
          const act = edit ? () => edit.onPlace(p.key) : live?.open?.[p.key];
          const look = w.kind === "persona" && w.persona ? lookOf(w.persona) : null;
          const label = p.key === "you" ? "You" : p.key;
          const isWorking = working.includes(p.key);
          const dots = (live?.items?.[p.key] ?? []).slice(0, MAX_DOTS);
          return (
            <Fragment key={p.key}>
            <g
              className={`org-node ${w.kind}${p.key === "coordinator" ? " coord" : ""}${edit?.picked === p.key ? " picked" : ""}${isWorking ? " working" : ""}${p.key === "you" && live?.waiting ? " waiting" : ""}`}
              data-hue={look?.hue}
              style={{ "--i": i } as CSSProperties}
              {...(act ? asButton(act) : {})}
            >
              <title>{isWorking ? `${w.title} (working)` : w.title}</title>
              {isWorking && <circle className="org-orb" cx={p.x} cy={p.y} r={p.r + 5} />}
              {isWorking && <circle className="org-spin" cx={p.x} cy={p.y} r={p.r + 7} />}
              {p.key === "coordinator" && <circle className="org-halo" cx={p.x} cy={p.y} r={p.r + 3.5} />}
              <circle className="org-shape" cx={p.x} cy={p.y} r={p.r} />
              {w.kind === "lead" && <circle className="org-nucleus" cx={p.x} cy={p.y} r={4} />}
              {w.kind === "helper" && <circle className="org-nucleus" cx={p.x} cy={p.y} r={2} />}
              {w.kind === "persona" && (
                <text className="org-letter" x={p.x} y={p.y} textAnchor="middle" dominantBaseline="central">
                  {look?.letter ?? "?"}
                </text>
              )}
              {w.kind === "you" ? (
                <text className="org-name inside" x={p.x} y={p.y} textAnchor="middle" dominantBaseline="central">
                  You
                </text>
              ) : (
                <text className="org-name" x={p.x} y={p.y + p.r + 12} textAnchor="middle">
                  {short(label, Math.floor(p.room / 6))}
                </text>
              )}
            </g>
            {/* The items this member has, as the dots the style's strip uses: a ring waiting, a
                dashed one being checked, a filled one done. Beside the member, never a count. */}
            {dots.map((d, i) => (
              <g key={`${p.key}-item-${d.n}`} className={`org-node item ${d.state}`}>
                <title>{`Item ${d.n}, ${d.word}: ${d.text}`}</title>
                <circle className="org-shape" cx={p.x + (i - (dots.length - 1) / 2) * 10} cy={p.y + p.r + 23} r={3.4} />
              </g>
            ))}
            </Fragment>
          );
        })}
        {live?.gate && gateAt && (
          <g className="org-node gate">
            <title>A second look before the answer goes out</title>
            <path
              className="org-shape"
              d={`M${gateAt.x} ${gateAt.y - 8} L${gateAt.x + 8} ${gateAt.y} L${gateAt.x} ${gateAt.y + 8} L${gateAt.x - 8} ${gateAt.y} Z`}
            />
            <text className="org-note" x={gateAt.x + 13} y={gateAt.y} dominantBaseline="central">
              check
            </text>
          </g>
        )}
        {step && flight && <Flight key={step.id} d={flight.d} reverse={flight.reverse} to={flight.to} hue={hueOf(step.from)} />}
      </svg>
      {sentence && (
        <p className={`org-sentence${live?.tone ? ` ${live.tone}` : ""}`} role="status">
          {sentence}
        </p>
      )}
    </section>
  );
}

/** How long a post takes to travel its line, and the comet's tail behind it (as parts of the way). */
const FLY_MS = 760;
const TAIL = [0.05, 0.1, 0.15];

const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

/**
 * `TEAM-16`: one post on its way. The line lights up behind it in the sender's hue, a small comet
 * runs from the sender to the receiver, and the receiver pings when it lands. It moves the SVG by
 * hand (no re-render per frame) and draws nothing under reduced motion or where paths cannot be
 * measured; the "Now:" sentence says the same thing in words.
 */
function Flight({ d, reverse, to, hue }: { d: string; reverse: boolean; to: Place; hue?: number }) {
  const path = useRef<SVGPathElement>(null);
  const trace = useRef<SVGPathElement>(null);
  const dots = useRef<(SVGCircleElement | null)[]>([]);
  const [phase, setPhase] = useState<"off" | "flying" | "landed">("off");

  useEffect(() => {
    const el = path.current;
    const reduce = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!el || typeof el.getTotalLength !== "function" || typeof requestAnimationFrame !== "function" || reduce) return;
    let len = 0;
    try {
      len = el.getTotalLength();
    } catch {
      return;
    }
    if (!len) return;
    setPhase("flying");
    const start = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / FLY_MS);
      const e = ease(t);
      const along = (k: number) => el.getPointAtLength(len * (reverse ? 1 - k : k));
      [0, ...TAIL].forEach((lag, i) => {
        const c = dots.current[i];
        if (!c) return;
        const pt = along(Math.max(0, e - lag));
        c.setAttribute("cx", pt.x.toFixed(1));
        c.setAttribute("cy", pt.y.toFixed(1));
      });
      if (trace.current) {
        trace.current.style.strokeDasharray = `${len} ${len}`;
        trace.current.style.strokeDashoffset = String((reverse ? -1 : 1) * len * (1 - e));
      }
      if (t < 1) frame = requestAnimationFrame(tick);
      else setPhase("landed");
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [reverse]);

  return (
    <g className={`org-flight ${phase}`} data-hue={hue} aria-hidden="true">
      <path ref={path} className="org-flight-path" d={d} />
      {phase !== "off" && <path ref={trace} className="org-flight-trace" d={d} />}
      {phase === "flying" &&
        [0, ...TAIL].map((_, i) => (
          <circle
            key={i}
            ref={(c) => {
              dots.current[i] = c;
            }}
            className={i === 0 ? "org-flight-head" : "org-flight-tail"}
            r={i === 0 ? 4.2 : 3.4 - i * 0.7}
            style={i > 0 ? { opacity: 0.55 - i * 0.14 } : undefined}
            cx={-20}
            cy={-20}
          />
        ))}
      {phase === "landed" && <circle className="org-ping" cx={to.x} cy={to.y} r={to.r + 2} />}
    </g>
  );
}

/** Where a task is, drawn the way the chart draws an item: an empty ring waiting, a ring with a
 * turning arc in hand, a dashed ring being checked or sent back, a filled ring with a tick done,
 * a crossed ring dropped or stopped. Ink only; the word beside it says the same. */
export function TaskMark({ state }: { state: TaskState }) {
  return (
    <svg className={`team-task-mark ${state}`} width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <circle className="ring" cx="8" cy="8" r="6.2" />
      {state === "doing" && <circle className="arc" cx="8" cy="8" r="6.2" />}
      {state === "done" && <path className="tick" d="M5 8.2l2 2 4-4.4" />}
      {state === "sent-back" && <circle className="dot" cx="8" cy="8" r="2" />}
      {(state === "dropped" || state === "stopped") && <path className="cross" d="M5.6 10.4l4.8-4.8" />}
    </svg>
  );
}
