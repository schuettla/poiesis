import type { AgentStep } from "./types";

/** `UI-2`: what one step says it does, as "verb target" ("searching web competitor pricing").
 * The run meter, the timeline head and the sidebar all use these words, so they never differ. */
export function stepWords(step: Pick<AgentStep, "verb" | "target">): string {
  return `${step.verb} ${step.target}`.trim();
}

/** `UI-2`: the first `max` characters, cut at a word, with an ellipsis when something was left. */
export function cutAtWord(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  const cut = flat.slice(0, max);
  const at = cut.lastIndexOf(" ");
  return `${(at > 0 ? cut.slice(0, at) : cut).trimEnd()}…`;
}

/** `UI-3`/`UI-4`: a clock, `m:ss`. */
export function clockText(ms: number): string {
  const secs = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/** `UI-4`: how long a step took, as omher writes it: "0.4s", "38s", "2m 41s". */
export function stepTime(ms: number): string {
  if (ms < 1000) return `${(Math.max(0, ms) / 1000).toFixed(1)}s`;
  const secs = Math.round(ms / 1000);
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}m ${secs % 60}s`;
}

/** Enough of a plan item to recognise it. The whole thing belongs on the plan card, not in a
 * one-line status that also has to carry a clock. */
export function trimItem(text: string, max = 44): string {
  return text.length > max ? `${text.slice(0, max).trimEnd()}…` : text;
}

/** `PLN-UI-2`/`UI-4`: what the live run is doing now, in the order of confidence: a tool that is
 * running, then thinking that is arriving, then the honest fallback. The plan item that is
 * being worked on goes in front, because it says what the work is for. */
export function liveActivity(
  steps: Pick<AgentStep, "status" | "verb" | "target">[] | undefined,
  run: { thinking: string; plan?: { items: { status: string; text: string }[] } } | null | undefined
): string {
  const running = steps?.find((s) => s.status === "running");
  const activity = running ? stepWords(running) : run?.thinking ? "thinking" : "waiting for the model";
  const item = run?.plan?.items.find((i) => i.status === "doing");
  return item ? `${trimItem(item.text)} · ${activity}` : activity;
}

/** `UI-4`: how long a finished run's steps took together: the first start to the last end.
 * Null when any step has no times, as a step read back from a saved message has none. */
export function stepsDuration(steps: Pick<AgentStep, "startedAt" | "endedAt">[]): number | null {
  if (steps.length === 0 || steps.some((s) => s.startedAt === undefined || s.endedAt === undefined)) return null;
  return Math.max(...steps.map((s) => s.endedAt!)) - Math.min(...steps.map((s) => s.startedAt!));
}
