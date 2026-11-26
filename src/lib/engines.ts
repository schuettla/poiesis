// The words Settings → External agents uses about an agent app (`CLI-UI-1`).
// Kept apart from the screen so each sentence can be tested on its own and the
// copy stays in one place (Part VII of the plan).

import type { EngineInfo, EnginePaidBy, EngineRoutePreview, EngineTryResult, EngineUseMode, PlanUsageView, RunEvidence } from "./api";
import { sureWord } from "./decisions";

/** The wording for who pays. */
export function paidByLabel(p: EnginePaidBy | null, model?: string | null): string | null {
  switch (p) {
    case "plan":
      return "on your plan";
    case "api-key":
      return "with your API key";
    case "own":
      return model && /-free$/.test(model) ? "free model" : "own provider";
    case "free":
      return "free model";
    default:
      return null;
  }
}

/** Where an agent's mark is (`XA-5`), built with the base path so it works when the
 * app is served from a subfolder. `null`: `AppLogo` shows the first letter. */
export function agentLogoUrl(logo: string | null | undefined): string | null {
  return logo ? `${import.meta.env.BASE_URL}agent-logos/${logo}` : null;
}

const DAY = 24 * 3600 * 1000;

/** `14:30` today, `Tue 14:30` another day. Local time. */
export function planWhen(ms: number, now: number = Date.now()): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return d.toDateString() === new Date(now).toDateString()
    ? clock
    : `${d.toLocaleDateString(undefined, { weekday: "short" })} ${clock}`;
}

/** `PRF-2`: the one line under a finished agent-app job that changed files. Built here from
 * `state`, `changed` and `check`, so it reads the same as the report the lead was given. */
export function evidenceLine(e: RunEvidence): string {
  const files = `Changed ${e.changed} file${e.changed === 1 ? "" : "s"}.`;
  const check = e.check ?? "the check";
  switch (e.state) {
    case "passed":
      return `${files} Ran ${check} after the last change: passed.`;
    case "failed":
      return `${files} Ran ${check} after the last change: failed.`;
    case "before-last":
      return `${files} Ran ${check}, but not after its last change.`;
    case "not-run":
      return `${files} It did not run the project's check (${check}).`;
    default:
      return files;
  }
}

/** `PRF-2`: the failed and not-run cases read in a warning tone. */
export function evidenceWarns(e: RunEvidence): boolean {
  return e.state === "failed" || e.state === "not-run";
}

/** `PRF-4`: what the change left open, for the line under the evidence: at most two items, then
 * "and N more"; `full` is every item, for the tooltip. `null` when nothing was left. */
export function evidenceLeft(e: RunEvidence): { text: string; full: string } | null {
  const left = e.left ?? [];
  if (left.length === 0) return null;
  const shown = left.slice(0, 2).join(", ");
  const more = left.length > 2 ? ` and ${left.length - 2} more` : "";
  return { text: `Left: ${shown}${more}.`, full: left.join("\n") };
}

/** `XA-13`/`XA-14`: the one word that picks a Claude Code (`cc-`) or Codex (`cx-`) session in
 * `/resume`: the last 8 characters of its id. Built the same way in the backend (`past::token`).
 * `null` for any other agent. */
export function outsideToken(engineId: string, session: string): string | null {
  const prefix = engineId === "claude-code" ? "cc" : engineId === "codex" ? "cx" : null;
  return prefix && session ? `${prefix}-${session.slice(-8)}` : null;
}

/** `XA-14`: how long ago a session was last worked in: "just now", "5 min ago", "2 h ago",
 * "yesterday", "3 days ago". */
export function sessionAge(ms: number, now: number = Date.now()): string {
  const min = Math.floor((now - ms) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.floor(h / 24);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

/** "(seen yesterday)" once what it said is more than a day old, else "". */
export function planSeen(plan: PlanUsageView, now: number = Date.now()): string {
  const days = Math.floor((now - plan.at_ms) / DAY);
  if (days < 1) return "";
  return days === 1 ? "(seen yesterday)" : `(seen ${days} days ago)`;
}

/** The next reset that is still ahead, if the app said one. */
function nextReset(plan: PlanUsageView, now: number): number | null {
  const times = [...plan.windows.map((w) => w.resets_at_ms), plan.resets_at_ms].filter(
    (t): t is number => t != null && t > now,
  );
  return times.length ? Math.min(...times) : null;
}

/** `XA-10`: the line under an agent app on the Usage page. */
export function planLine(plan: PlanUsageView, now: number = Date.now()): { text: string; tone: "plain" | "warn" } | null {
  const seen = planSeen(plan, now);
  const end = seen ? ` ${seen}` : "";
  if (plan.reached) {
    const back = plan.resets_at_ms != null && plan.resets_at_ms > now ? ` Back at ${planWhen(plan.resets_at_ms, now)}.` : "";
    return { text: `Plan limit reached.${back}${end}`, tone: "warn" };
  }
  if (plan.windows.length === 0) return null;
  const parts = plan.windows.map((w) => {
    const of = w.name === "week" || w.name === "month" ? `the ${w.name}` : w.name;
    return `${Math.round(w.used * 100)}% of ${of}`;
  });
  const reset = nextReset(plan, now);
  return { text: `Plan: ${parts.join(", ")}.${reset != null ? ` Resets ${planWhen(reset, now)}.` : ""}${end}`, tone: "plain" };
}

export interface EngineStatus {
  text: string;
  /** `warn` is amber: it works, but check. `bad` is for what stops it. */
  tone: "plain" | "warn" | "bad";
}

/** The one status line under an agent's name. */
export function engineStatus(e: EngineInfo): EngineStatus {
  if (!e.found) {
    return {
      text: e.last_error ? "I can't find it any more." : "I didn't find it on this computer.",
      tone: "plain",
    };
  }
  const parts: string[] = [];
  if (e.version) parts.push(e.version);
  if (e.signed_in === true) parts.push("signed in");
  if (e.signed_in === false) parts.push("not signed in");
  const pay =
    e.payment === "api-key" ? "with your API key" : paidByLabel(e.paid_by, e.model);
  if (pay && e.signed_in !== false) parts.push(pay);
  // `XA-10`: the plan, when it was said today. A day-old figure says nothing about now.
  const plan = e.plan && Date.now() - e.plan.at_ms < DAY ? e.plan : null;
  if (plan?.reached) parts.push("plan limit reached");
  else if (plan && plan.windows.length > 0) {
    parts.push(`${Math.round(Math.max(...plan.windows.map((w) => w.used)) * 100)}% of plan used`);
  }
  if (e.tested === "newer") parts.push("newer than I've been tested with");
  if (e.tested === "older") parts.push("older than I've been tested with");
  if (parts.length === 0 && e.last_error) return { text: e.last_error, tone: "bad" };
  const tone =
    e.signed_in === false ? "bad" : e.tested === "newer" || e.tested === "older" || plan?.reached ? "warn" : "plain";
  return { text: parts.join(" · "), tone };
}

/** Why the switch cannot be turned on yet, or null when it can. */
export function cannotEnable(e: EngineInfo): string | null {
  if (!e.found) return "I can't find it on this computer.";
  if (e.payment === "plan" && e.signed_in === false) return "Sign in first.";
  return null;
}

/** The line **Try it** leaves behind. */
export function tryLine(r: EngineTryResult): string {
  if (!r.ok) return r.error ?? "It did not work.";
  const parts = [`Ready in ${r.seconds.toFixed(1)} s`];
  const pay = paidByLabel(r.paid_by, r.model);
  if (pay) parts.push(pay);
  if (r.model) parts.push(r.model);
  return parts.join(" · ");
}

/** The short chip on a Fleet row: who paid for the job. */
export function payChip(p: EnginePaidBy | null | undefined): string | null {
  return p === "api-key" ? "API key" : paidByLabel(p ?? null);
}

/** The "why" chip on a Fleet row. `asked` is the user naming the agent;
 * `persona` is a persona the user set to run on this agent app. */
export function becauseChip(because: string | null | undefined): string | null {
  if (!because) return null;
  if (because === "asked") return "you asked";
  if (because === "persona") return "its persona";
  return `because ${because}`;
}

/** `USE-UI-10`: the rule line of a handover card. One a quick decision matched
 * before I ran says so, in words (`SEE-4`): sure, or "probably". One I cited says that.
 * The number is `matchedTitle`, on the line's hover. */
export function ruleLine(rule: { label: string; text: string; matched?: number | null }): string {
  if (rule.matched == null) return `Because of your rule ${rule.label}: ${rule.text}`;
  // A match is an unfitted number to the card: it can only hedge more.
  const lead = sureWord(rule.matched, false) === "sure" ? "Matches" : "Probably matches";
  return `${lead} your rule ${rule.label}: ${rule.text}`;
}

/** The hover on the Fleet row's dot and on a handover card's rule line. */
export function matchedTitle(matched: number): string {
  return `matched before I started (${matched.toFixed(2)})`;
}

/** **Try a request**: which path answered, so it is clear whether the quick
 * decision or my chat model did. */
export function answeredBy(p: Pick<EngineRoutePreview, "quick_ms">): string {
  return p.quick_ms == null ? "asked my chat model" : `quick decision · ${p.quick_ms} ms`;
}

/** The muted counter under a rule once quick decisions have matched it. */
export function matchCounter(c: { matched: number; declined: number } | undefined): string | null {
  if (!c) return null;
  const parts: string[] = [];
  if (c.matched > 0) parts.push(`matched early ${c.matched}×`);
  if (c.declined > 0) parts.push(`you said do it yourself ${c.declined}×`);
  return parts.length ? parts.join(" · ") : null;
}

export const MODE_LABELS: Record<EngineUseMode, string> = {
  on_request: "Only when you ask",
  suggest: "Suggest it, you confirm",
  auto: "On its own",
};

/** The help line under the mode control; it changes with the choice. */
export function modeHelp(mode: EngineUseMode, name: string, id: string): string {
  switch (mode) {
    case "on_request":
      // What is typed after /delegate: the id without its protocol prefix.
      return `I won't pick it myself. Use /delegate ${id.replace(/^acp:/, "")} … when you want it.`;
    case "suggest":
      return "I'll suggest it when a rule fits, and you confirm each time.";
    case "auto":
      return `I'll hand work to ${name} when a rule fits, and tell you which rule.`;
  }
}

/** **Try a request**: the answer in one line. */
export function previewLine(p: EngineRoutePreview): string {
  if (!p.agent || !p.label) return "I'd do this myself. No rule fits.";
  if (p.blocked) return `I'd want ${p.agent} (${p.label}), but ${p.blocked}`;
  switch (p.consequence) {
    case "only_when_asked":
      return `I'd do this myself. Rule ${p.label} fits ${p.agent}, but it is set to only when you ask.`;
    case "start":
      return `I'd hand this to ${p.agent}, because of ${p.label}. It would start at once.`;
    default:
      return `I'd hand this to ${p.agent}, because of ${p.label}. I'd ask you first.`;
  }
}

/** A rule declined more often than it was used is probably wrong. */
export function ruleLooksWrong(r: { hits: number; overrides: number }): boolean {
  return r.overrides > r.hits;
}

/** One variable per row in the form; the backend wants a map. */
export function envToMap(rows: { name: string; value: string }[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) {
    const name = r.name.trim();
    if (name) out[name] = r.value;
  }
  return out;
}

/** One argument per line in the form; the backend wants a list. */
export function linesToArgs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
}
