import type { Persona, Team, TeamLine, TeamLineKind, TeamMember, TeamShape } from "./api";

/**
 * `TEAM-9`: how a team is drawn. Pure functions, so the chart in the Teams tab and (`TEAM-14`)
 * the live Coordination view place the same people in the same places.
 *
 * The layout is automatic per shape and nobody drags anything (`TEAMS_PLAN` IV.2): the
 * coordinator is under You; the members are in a row; a reviewer sits under what it reviews; a
 * pipeline runs left to right; Open puts everyone around one table.
 */

export const CHART_W = 480;
export const CHART_H = 240;
const R = 13;
const MIN_GAP = 70;

export interface Place {
  /** `you`, `coordinator` or a member's role. */
  key: string;
  x: number;
  y: number;
  r: number;
  /** How wide its name may be, in px, before the next place. */
  room: number;
}

/** `n` places in a row centred on `cx`, as far apart as `step` and as wide as `span` allow. */
function row(n: number, cx: number, span: number, step: number): { xs: number[]; step: number } {
  const s = n <= 1 ? step : Math.min(step, span / (n - 1));
  const left = cx - ((n - 1) * s) / 2;
  return { xs: Array.from({ length: n }, (_, i) => left + i * s), step: s };
}

/** Members a reviewer looks at, by role. */
const reviewing = (lines: TeamLine[], who: string) => lines.filter((l) => l.kind === "reviews" && l.from === who).map((l) => l.to);

/** A member the coordinator hands no work to, and who reviews others: it sits under them. */
function isReviewer(t: Pick<Team, "lines">, role: string): boolean {
  return reviewing(t.lines, role).length > 0 && !t.lines.some((l) => l.kind === "hands-over" && l.from === "coordinator" && l.to === role);
}

export function layoutTeam(t: Pick<Team, "shape" | "members" | "lines">): Place[] {
  const ms = t.members;
  const out: Place[] = [];
  const put = (key: string, x: number, y: number, room = 60) => out.push({ key, x, y, r: R, room });

  if (t.shape === "pipeline") {
    put("you", 40, 30);
    put("coordinator", 40, 112, 70);
    const left = 124;
    const step = ms.length <= 1 ? 100 : Math.min(100, (444 - left) / (ms.length - 1));
    ms.forEach((m, i) => put(m.role, left + i * step, 112, step * 0.95));
  } else if (t.shape === "open") {
    put("you", CHART_W / 2, 24);
    const all = ["coordinator", ...ms.map((m) => m.role)];
    all.forEach((key, i) => {
      const angle = -Math.PI / 2 + (2 * Math.PI * i) / all.length;
      put(key, CHART_W / 2 + 180 * Math.cos(angle), 132 + 64 * Math.sin(angle), 70);
    });
  } else {
    // Hub and Review: You, the coordinator, the workers in a row, the reviewers under them.
    const reviewers = ms.filter((m) => isReviewer(t, m.role));
    const workers = ms.filter((m) => !reviewers.includes(m));
    const rowY = reviewers.length ? 136 : 150;
    put("you", CHART_W / 2, 24);
    put("coordinator", CHART_W / 2, 80, 100);
    const { xs, step } = row(workers.length, CHART_W / 2, 400, 100);
    workers.forEach((m, i) => put(m.role, xs[i], rowY, step * 0.95));
    // Each reviewer under the middle of what it reviews; two never overlap.
    let prev = -Infinity;
    reviewers
      .map((m) => {
        const under = reviewing(t.lines, m.role)
          .map((r) => out.find((p) => p.key === r)?.x)
          .filter((x): x is number => x !== undefined);
        return { m, x: under.length ? under.reduce((a, b) => a + b, 0) / under.length : CHART_W / 2 };
      })
      .sort((a, b) => a.x - b.x)
      .forEach((r) => {
        const x = Math.max(r.x, prev + MIN_GAP);
        prev = x;
        put(r.m.role, Math.min(x, CHART_W - 40), 204, MIN_GAP);
      });
  }
  return out;
}

/** One line on the chart: everything that runs between the same two places. */
export interface ChartPair {
  key: string;
  a: string;
  b: string;
  lines: TeamLine[];
  /** What it is drawn as: the strongest of its lines. */
  kind: TeamLineKind;
  /** Some line runs from a to b / from b to a. */
  toB: boolean;
  toA: boolean;
}

const STRENGTH: TeamLineKind[] = ["hands-over", "reviews", "asks", "talks", "reports"];

/** The lines, one drawn line per two places. Open has two or four lines between everyone and
 * still draws one, with an arrowhead at each end that has one. */
export function pairsOf(lines: TeamLine[]): ChartPair[] {
  const out = new Map<string, ChartPair>();
  for (const l of lines) {
    const [a, b] = [l.from, l.to].sort();
    const key = `${a}|${b}`;
    const pair = out.get(key) ?? { key, a, b, lines: [], kind: l.kind, toB: false, toA: false };
    pair.lines.push(l);
    if (STRENGTH.indexOf(l.kind) < STRENGTH.indexOf(pair.kind)) pair.kind = l.kind;
    if (l.to === b) pair.toB = true;
    else pair.toA = true;
    out.set(key, pair);
  }
  return [...out.values()];
}

const dist = (p: { x: number; y: number }, a: { x: number; y: number }, b: { x: number; y: number }): number => {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy || 1;
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
};

const rim = (from: Place, toward: { x: number; y: number }, gap = 3) => {
  const dx = toward.x - from.x;
  const dy = toward.y - from.y;
  const len = Math.hypot(dx, dy) || 1;
  return { x: from.x + (dx / len) * (from.r + gap), y: from.y + (dy / len) * (from.r + gap) };
};
const f = (n: number) => n.toFixed(1);

/** The line between two places, rim to rim. Straight, unless it would pass through someone
 * else: then it bows to the lower side. */
export function edgePath(a: Place, b: Place, others: Place[]): string {
  const blocked = others.some((o) => dist(o, a, b) < o.r + 5);
  if (!blocked) {
    const s = rim(a, b);
    const e = rim(b, a);
    return `M${f(s.x)} ${f(s.y)} L${f(e.x)} ${f(e.y)}`;
  }
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy) || 1;
  let nx = -dy / len;
  let ny = dx / len;
  if (ny < 0 || (ny === 0 && nx < 0)) {
    nx = -nx;
    ny = -ny;
  }
  const bend = Math.min(80, 30 + len * 0.2);
  const c = { x: (a.x + b.x) / 2 + nx * bend, y: (a.y + b.y) / 2 + ny * bend };
  const s = rim(a, c);
  const e = rim(b, c);
  return `M${f(s.x)} ${f(s.y)} Q${f(c.x)} ${f(c.y)} ${f(e.x)} ${f(e.y)}`;
}

/* ---- lines in words ---- */

const VERB: Record<TeamLineKind, string> = {
  "hands-over": "hands over work to",
  reviews: "reviews",
  asks: "may ask",
  reports: "reports to",
  talks: "can talk with",
};

/** What a line is called in the small menu: "hands over work to", "reviews", "may ask",
 * "reports to", and for You "can talk with". */
export const lineVerb = (kind: TeamLineKind): string => VERB[kind];

/** The kinds of line that make sense between two places (the same rule the file reader keeps):
 * You only talks, or is reported to; nobody has a line to themselves. */
export function lineKindsFor(from: string, to: string): TeamLineKind[] {
  if (from === to) return [];
  if (from === "you") return ["talks"];
  if (to === "you") return ["reports"];
  return ["hands-over", "reviews", "asks", "reports"];
}

/** A name for each place, as a sentence wants it: the persona that coordinates (or Poiesis), a
 * member's role, and You. */
export type Namer = (who: string, first: boolean) => string;

export function teamNamer(team: Pick<Team, "coordinator">, personas: Pick<Persona, "id" | "name">[]): Namer {
  const coordinator =
    team.coordinator === "poiesis" ? "Poiesis" : (personas.find((p) => `persona:${p.id}` === team.coordinator)?.name ?? "the coordinator");
  return (who, first) => (who === "you" ? (first ? "You" : "you") : who === "coordinator" ? coordinator : who);
}

/** "verifier reviews api's work", "Mira hands over work to api", "You can talk with Mira". */
export function lineWords(l: TeamLine, name: Namer): string {
  const from = name(l.from, true);
  const to = name(l.to, false);
  return l.kind === "reviews" ? `${from} reviews ${to}'s work` : `${from} ${VERB[l.kind]} ${to}`;
}

export const lineKey = (l: TeamLine): string => `${l.from} ${l.kind} ${l.to}`;

/** The two lines nobody can take away. */
export const isFixedLine = (l: TeamLine): boolean =>
  (l.kind === "reports" && l.from === "coordinator" && l.to === "you") || (l.kind === "talks" && l.from === "you" && l.to === "coordinator");

/** The same lines, whatever the order. */
export function sameLines(a: TeamLine[], b: TeamLine[]): boolean {
  const ka = new Set(a.map(lineKey));
  const kb = new Set(b.map(lineKey));
  return ka.size === kb.size && [...ka].every((k) => kb.has(k));
}

/* ---- members ---- */

export const SHAPES: { id: TeamShape; name: string; hint: string }[] = [
  { id: "hub", name: "Hub", hint: "The coordinator hands work to every member." },
  { id: "pipeline", name: "Pipeline", hint: "Work goes from one member to the next, left to right." },
  { id: "review", name: "Review", hint: "The last member reviews the work of the others." },
  { id: "open", name: "Open", hint: "Everyone may hand work to and ask everyone." },
];

/** Where a new member goes: last, except in Review, where the last one is the reviewer. */
export function withMember(shape: TeamShape, members: TeamMember[], m: TeamMember): TeamMember[] {
  if (shape === "review" && members.length >= 2) return [...members.slice(0, -1), m, members[members.length - 1]];
  return [...members, m];
}

/** The lines, less every one that has `role` in it. */
export const withoutRole = (lines: TeamLine[], role: string): TeamLine[] => lines.filter((l) => l.from !== role && l.to !== role);

/** `have` and what `add` has that `before` did not: a new member brings only its own lines,
 * and the ones the person drew stay. */
export function withNewLines(have: TeamLine[], before: TeamLine[], add: TeamLine[]): TeamLine[] {
  const known = new Set([...have, ...before].map(lineKey));
  return [...have, ...add.filter((l) => !known.has(lineKey(l)))];
}

/** A role name made from a persona's name: small letters, digits, dashes, at most 24 letters. */
export function roleFrom(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 24);
}

const RESERVED = ["coordinator", "you", "all"];

/** Why a role name cannot be used, or `null`. The rule the saved file keeps. */
export function roleProblem(role: string, taken: string[]): string | null {
  if (!role) return "Give the place a name.";
  if (RESERVED.includes(role)) return `${role} is taken: it means someone else in a line.`;
  if (!/^[a-z0-9_-]{1,24}$/.test(role)) return "Use small letters, digits and dashes, up to 24.";
  if (taken.includes(role)) return `Two members cannot both be called ${role}.`;
  return null;
}
