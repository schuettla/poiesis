import type { DecisionEntry, PlanView, Thought, VerifyCheck } from "./api";

/** `SEE-2`: how the first-thought strip and the receipt word what the quick
 * decisions did. Pure, so the words can be tried without a screen. The rule
 * (`SEE-0`): what I will do or did, about the person's task; numbers only on a hover. */

/** What a turn keeps of its quick decisions, in memory only: it is about a moment. */
export interface TurnThought {
  runId: string;
  /** When the run started and, once it ended, when. The receipt reads the log by these. */
  from: number;
  to?: number;
  thought: Thought;
  /** Groups the model or the person put back on the menu. */
  back: string[];
  /** The decisions of the turn, read from the log when the run ended. */
  decisions?: DecisionEntry[];
}

/** A turn that has not said anything yet. */
export const emptyThought = (): Thought => ({
  looking: [],
  direct: false,
  left_out: [],
  held: [],
  handover: null,
  ms: 0,
  engine: null,
});

/** `a`, `a and b`, `a, b and c`. */
export function joinWords(parts: string[]): string {
  if (parts.length <= 1) return parts[0] ?? "";
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

const lower = (s: string) => s.charAt(0).toLowerCase() + s.slice(1);

export interface StripPart {
  kind: "where" | "left-out" | "who" | "speed";
  text: string;
  /** The click that puts a group back: its words and the group. */
  action?: { label: string; unit: string };
}

/** The parts, in order, each only when it applies. `back` is what already returned
 * to the menu, which is no longer left out. `Looking in Google Calendar`, `Not
 * searching the web`, `Claude Code's job, by your rule U2`, `38 ms`. */
export function stripParts(t: Thought, back: string[] = []): StripPart[] {
  const parts: StripPart[] = [];
  if (t.looking.length) parts.push({ kind: "where", text: `Looking ${joinWords(t.looking)}` });
  else if (t.direct) parts.push({ kind: "where", text: "Answering directly" });
  for (const out of t.left_out.filter((l) => !back.includes(l.id))) {
    parts.push({ kind: "left-out", text: parts.length ? lower(out.words) : out.words, action: { label: out.action, unit: out.id } });
  }
  if (t.handover) parts.push({ kind: "who", text: `${t.handover.agent}'s job, by your rule ${t.handover.rule}` });
  if (parts.length && t.ms > 0) parts.push({ kind: "speed", text: `${t.ms} ms` });
  return parts;
}

/** The line for a decision engine that was not there to ask, saying why as it is. `start`
 * when a Start makes sense (installed, not running yet); `choose` when the fix is picking a
 * decision model, a cloud one included (none set up, or one on the processor, which a
 * waiting turn does not wait for). */
export function engineLine(t: Thought): { text: string; start: boolean; choose: boolean } | null {
  if (!t.engine) return null;
  switch (t.engine) {
    case "cold":
      return { text: "Quick thinking is starting: the decision model isn't running yet", start: true, choose: false };
    case "cpu":
      return {
        text: "Quick thinking is off for this: the decision model runs on the processor, too slow to wait for",
        start: false,
        choose: true,
      };
    default:
      return { text: "Quick thinking is off: no decision model is set up", start: false, choose: true };
  }
}

/** What the held-back groups would have cost the prompt, for the ones that stayed out. */
export function tokensShorter(t: Thought, back: string[]): number {
  return t.held.filter((h) => !back.includes(h.id)).reduce((sum, h) => sum + h.tokens, 0);
}

export interface ReceiptRow {
  id: string;
  text: string;
  /** Backend, model, how sure and how long: numbers belong here. */
  hover: string;
  purpose: string;
  /** Already marked wrong from the receipt. */
  wrong: boolean;
  /** The groups to name when this is the tool-menu row. */
  units: string[];
  /** A plan step this decision ticked, so the tick can be reopened. */
  step: number | null;
  /** The one row that sums up the check of the answer's sentences; it has no decision of its own. */
  summary?: boolean;
}

export interface Receipt {
  line: string;
  rows: ReceiptRow[];
  /** Answered decisions that changed nothing. */
  quiet: number;
}

const sureOf = (d: DecisionEntry): number | null => {
  const ps = Object.values(d.answers ?? {})
    .map((a) => a.confidence ?? a.p)
    .filter((p): p is number => typeof p === "number");
  return ps.length ? Math.max(...ps) : null;
};

/** `plan step 3` from the note `Looks finished from the answer: plan step 3`. */
const stepOf = (d: DecisionEntry, plan: PlanView | undefined): number | null => {
  if (d.purpose !== "plan-done" || !plan) return null;
  const n = Number(/plan step (\d+)$/.exec(d.note ?? "")?.[1]);
  const index = n - 1;
  return Number.isInteger(index) && plan.items[index]?.status === "done" ? index : null;
};

/** The questions of the check on each sentence (`SEE-1`). They are counted by the sentences
 * part of the line, not as decisions of their own: a long answer asks dozens. */
const CHECK_PURPOSES = ["claim-checkable", "claim-verdict"];

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** `6 quick decisions · prompt 2,900 tokens shorter · 9 sentences checked`, and a row for each
 * decision that acted or was shown. `null` when there is nothing to report. Only measured
 * facts: the count is from the log, the tokens from the held-back specs, the sentences from
 * the check of the answer. */
export function receiptOf(t: TurnThought, plan?: PlanView, check?: VerifyCheck | null): Receipt | null {
  const decisions = t.decisions ?? [];
  const others = decisions.filter((d) => !CHECK_PURPOSES.includes(d.purpose));
  const summary = check?.mode === "auto" ? check.summary : null;
  const checked = summary?.answered ?? 0;
  if (others.length === 0 && checked === 0) return null;
  const parts: string[] = [];
  if (others.length > 0) parts.push(plural(others.length, "quick decision", "quick decisions"));
  const shorter = tokensShorter(t.thought, t.back);
  if (shorter > 0) parts.push(`prompt ${shorter.toLocaleString()} tokens shorter`);
  if (checked > 0) parts.push(`${plural(checked, "sentence", "sentences")} checked`);
  const rows = decisions
    .filter((d) => !!d.note)
    .map<ReceiptRow>((d) => {
      const sure = sureOf(d);
      return {
        id: d.id,
        text: d.note as string,
        hover: [d.backend, d.model, d.ms > 0 ? `${d.ms} ms` : null, sure === null ? null : `sure ${sure.toFixed(2)}`]
          .filter(Boolean)
          .join(" · "),
        purpose: d.purpose,
        wrong: d.labels.some((l) => l.source.startsWith("receipt")),
        units: d.purpose === "tool-select" ? t.thought.held.map((h) => h.id).filter((id) => !t.back.includes(id)) : [],
        step: stepOf(d, plan),
      };
    });
  if (summary && checked > 0 && check) {
    const wrong = check.claims.filter((c) => c.finding === "refuted").length;
    const against = summary.sources > 0 ? `against ${plural(summary.sources, "source", "sources")}` : "against this run's steps";
    rows.push({
      id: "checked-sentences",
      text: `Checked ${plural(checked, "sentence", "sentences")} ${against}${wrong > 0 ? `, ${wrong} contradicted` : ""}`,
      hover: "",
      purpose: "claim-checkable",
      wrong: false,
      units: [],
      step: null,
      summary: true,
    });
  }
  return { line: parts.join(" · "), rows, quiet: others.filter((d) => !d.note).length };
}
