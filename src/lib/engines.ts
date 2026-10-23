// The words Settings → External agents uses about an agent app (`CLI-UI-1`).
// Kept apart from the screen so each sentence can be tested on its own and the
// copy stays in one place (Part VII of the plan).

import type { EngineInfo, EnginePaidBy, EngineRoutePreview, EngineTryResult, EngineUseMode } from "./api";

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
  if (e.tested === "newer") parts.push("newer than I've been tested with");
  if (e.tested === "older") parts.push("older than I've been tested with");
  if (parts.length === 0 && e.last_error) return { text: e.last_error, tone: "bad" };
  const tone = e.signed_in === false ? "bad" : e.tested === "newer" || e.tested === "older" ? "warn" : "plain";
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
