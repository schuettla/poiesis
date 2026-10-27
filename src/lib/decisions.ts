import type { ActivityEntry, DecisionAnswer, DecisionEntry, DecisionLabel } from "./api";

/** `CTL-UI-1`: how the Activity screen words and groups decisions. Pure, so the
 * wording and the grouping can be tried without a screen. */

export interface Option {
  name: string;
  p: number;
}

const UNIT_NAMES: Record<string, string> = {
  web_search: "Web search",
  code_exec: "Code execution",
  code_run: "Run project tasks",
  image_gen: "Image generation",
  indexing: "Folder reading",
  apps: "Connected apps",
  subagents: "Other agents",
};

/** `USE-4`: a tool group by the words the screen uses. */
export function unitName(id: string): string {
  if (id.startsWith("mcp:")) return "A connector";
  return UNIT_NAMES[id] ?? (id.charAt(0).toUpperCase() + id.slice(1)).replace(/_/g, " ");
}

/** One yes-or-no question for each of several items (the tool menu asks about
 * each group), rather than one question with options. */
function perItem(d: Pick<DecisionEntry, "answers">): [string, number][] | null {
  const entries = Object.entries(d.answers ?? {});
  if (entries.length < 2 || !entries.every(([, a]) => typeof a.p === "number")) return null;
  return entries.map(([id, a]) => [id, a.p as number]);
}

/** The first answer's options with their probabilities, in the order the
 * question listed them. A yes-or-no question reads yes, no. A decision that asked
 * one yes-or-no question per item reads item by item: how likely each was true. */
export function optionsOf(d: Pick<DecisionEntry, "answers">): Option[] {
  const items = perItem(d);
  if (items) return items.map(([id, p]) => ({ name: unitName(id), p }));
  const first: DecisionAnswer | undefined = Object.values(d.answers ?? {})[0];
  if (!first) return [];
  if (typeof first.p === "number") {
    return [
      { name: "yes", p: first.p },
      { name: "no", p: 1 - first.p },
    ];
  }
  return (first.probs ?? []).map((item, i) =>
    Array.isArray(item) ? { name: item[0], p: item[1] } : { name: String(i + 1), p: item }
  );
}

const two = (p: number) => p.toFixed(2);

/** How sure a decision is, said in words (`SEE-4`). The number stays in the hover
 * and the detail card. */
export type Sure = "sure" | "probably";

/** A fitted probability is sure from here. */
export const SURE_CALIBRATED = 0.85;
/** An unfitted one has to be this high: nothing has checked what it means. */
export const SURE_UNCALIBRATED = 0.9;
/** Below this a decision did not act, so there is nothing to say. */
export const PROBABLY_FROM = 0.6;

/** Sure: stated plainly. Middling (0.6 up to the bar, and any unfitted value under
 * 0.9): "probably". Below that, `null`: it did not act and is not shown. The same rule
 * as `sure_word` in `decide/words.rs`. */
export function sureWord(p: number, calibrated: boolean): Sure | null {
  if (p >= (calibrated ? SURE_CALIBRATED : SURE_UNCALIBRATED)) return "sure";
  return p >= PROBABLY_FROM ? "probably" : null;
}

/** `plain` when sure, `hedged` when probably, `null` below the floor. */
export function says<T>(p: number, calibrated: boolean, plain: T, hedged: T): T | null {
  const sure = sureWord(p, calibrated);
  return sure === null ? null : sure === "sure" ? plain : hedged;
}

/** `Decided · <question> · yes 0.82, no 0.18 · 41 ms · this computer`. Long
 * lists show the three likeliest, then how many more. */
export function decisionLine(d: DecisionEntry): string {
  const options = optionsOf(d);
  // Per item, the ones held back are the ones with a low number, so those lead.
  const byLikeliest = (a: Option, b: Option) => (perItem(d) ? a.p - b.p : b.p - a.p);
  const shown = [...options].sort(byLikeliest).slice(0, 3);
  const more = options.length - shown.length;
  const probs = shown.map((o) => `${o.name} ${two(o.p)}`).join(", ") + (more > 0 ? ` and ${more} more` : "");
  const parts = ["Decided", d.label || d.purpose];
  if (probs) parts.push(probs);
  if (d.ms > 0) parts.push(`${d.ms} ms`);
  parts.push(whereText(d.backend));
  return parts.join(" · ");
}

export function whereText(backend: string): string {
  return backend.startsWith("local:") ? "this computer" : backend;
}

/** What the controller did, in words. Empty when nothing was said. */
export function actionText(action: string | null): string {
  if (!action) return "";
  const [kind, detail] = action.split(":");
  switch (kind) {
    case "acted":
      return "Acted on it";
    case "default":
      return "Not sure enough, so I took the usual way";
    case "blocked":
      return "A rule said no, whatever the answer";
    case "escalated":
      return `Not sure enough, so I escalated to ${
        { evidence: "more evidence", slow: "the chat model", user: "you" }[detail ?? ""] ?? "a slower check"
      }`;
    case "budget":
      return `Not asked: ${
        {
          decisions: "this turn had used its decisions",
          escalations: "this turn had used its escalations",
          repeat: "the same question about the same state was already asked",
          background: "background decisions had used this hour's budget",
          share: "the checks of outside text had used their half of this hour's budget",
          "mail-share": "mail triage had used its share of this hour's budget",
          classify: "this turn had sorted as many items as it may",
          claims: "this check had judged as many claims as it may",
          sentences: "this check had asked about as many sentences as it may",
          reviews: "this check had sent as many claims to review as it may",
        }[detail ?? ""] ?? "a limit was reached"
      }`;
    default:
      return action;
  }
}

const LABEL_WORDS: Record<DecisionLabel["label"], string> = {
  right: "Right",
  wrong: "Wrong",
  weak_right: "Probably right",
  weak_wrong: "Probably wrong",
  corrected: "Corrected",
};

/** `Wrong (you chose another agent)`, `Corrected to topical`. */
export function labelText(l: DecisionLabel): string {
  // `USE-4`: a label on one group the tool menu left off.
  if (l.value && /^(need-tools|called-held|chip):/.test(l.source)) return `Needed after all: ${unitName(l.value)}`;
  if (l.value && l.source.startsWith("kept-out:")) return `Probably not needed: ${unitName(l.value)}`;
  const word = LABEL_WORDS[l.label] ?? l.label;
  return l.label === "corrected" && l.value ? `${word} to ${l.value}` : word;
}

/** A decision that was not asked at all: a limit was hit before the engine. */
export const wasSkipped = (d: Pick<DecisionEntry, "answers">) => optionsOf(d).length === 0;

export type Row =
  | { kind: "activity"; key: string; at: number; entry: ActivityEntry }
  | { kind: "decision"; key: string; at: number; entry: DecisionEntry }
  | {
      kind: "group";
      key: string;
      at: number;
      /** The hour the group covers, as the start of that hour. */
      hour: number;
      purpose: string;
      label: string;
      entries: DecisionEntry[];
    };

const HOUR = 3_600_000;

/** One list, newest first: activity as it was, a foreground decision as its own
 * row, and background decisions folded into one row per hour and purpose
 * ("12 decisions · mail triage"). */
export function rowsOf(activity: ActivityEntry[], decisions: DecisionEntry[]): Row[] {
  const rows: Row[] = activity.map((entry) => ({ kind: "activity", key: `a:${entry.id}`, at: entry.created_at, entry }));
  const groups = new Map<string, Extract<Row, { kind: "group" }>>();
  for (const d of decisions) {
    if (!d.background) {
      rows.push({ kind: "decision", key: `d:${d.id}`, at: d.at, entry: d });
      continue;
    }
    const hour = Math.floor(d.at / HOUR) * HOUR;
    const key = `g:${d.purpose}:${hour}`;
    const g = groups.get(key);
    if (g) {
      g.entries.push(d);
      g.at = Math.max(g.at, d.at);
    } else {
      groups.set(key, { kind: "group", key, at: d.at, hour, purpose: d.purpose, label: d.label, entries: [d] });
    }
  }
  rows.push(...groups.values());
  return rows.sort((a, b) => b.at - a.at);
}

export type Filter = "all" | "activity" | "decisions" | `experiment:${string}`;

export function filterRows(rows: Row[], filter: Filter): Row[] {
  if (filter === "all") return rows;
  if (filter === "activity") return rows.filter((r) => r.kind === "activity");
  const wanted = filter.startsWith("experiment:") ? filter.slice("experiment:".length) : null;
  return rows.filter((r) => {
    if (r.kind === "activity") return false;
    if (wanted === null) return true;
    const entries = r.kind === "group" ? r.entries : [r.entry];
    return entries.some((d) => d.experiment === wanted);
  });
}

/** The experiments that own a decision on screen, for one filter chip each. */
export function experimentsIn(decisions: DecisionEntry[]): string[] {
  return [...new Set(decisions.map((d) => d.experiment).filter((e): e is string => !!e))];
}

/** "12 decisions · mail triage", and how many of them a budget stopped before
 * they were asked: "12 decisions · Is there anything to learn? · 5 not asked". */
export function groupLine(g: Extract<Row, { kind: "group" }>): string {
  const n = g.entries.length;
  const stopped = g.entries.filter((d) => d.action?.startsWith("budget:")).length;
  const base = `${n} ${n === 1 ? "decision" : "decisions"} · ${g.label || g.purpose}`;
  return stopped > 0 ? `${base} · ${stopped} not asked, over budget` : base;
}
