// What Poiesis did during a spoken turn, for the voice surface: the tool steps
// and the agents it started. In the chat these live in the timeline; voice mode
// covers the chat, so without this the user hears "let me look that up" and then
// nothing on screen says what happened.

import type { Message, SubRun } from "../types";

export type ActivityState = "running" | "done" | "error" | "stopped";

export interface ActivityRow {
  id: string;
  kind: "step" | "agent";
  /** What it is, in the words the timeline uses. */
  label: string;
  state: ActivityState;
  /** How it went, when it said ("3 matches", or why it failed). */
  detail?: string;
}

/** The rows the surface shows; older ones are only counted. */
export const ACTIVITY_SHOWN = 6;

function clip(text: string, max: number): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max - 1)}…` : one;
}

/** The step result reads "— 3 matches" in the timeline; the dash is its bullet. */
function cleanResult(result: string | undefined): string | undefined {
  const text = result?.replace(/^[—–-]\s*/, "").trim();
  return text ? clip(text, 80) : undefined;
}

/**
 * Rows for one assistant message, oldest first. `since` is when the voice
 * conversation began: an answer from before it is chat history, not activity.
 */
export function activityFor(
  message: Message | undefined,
  subRuns: Record<string, SubRun>,
  since: number
): ActivityRow[] {
  if (!message || message.role !== "assistant" || message.createdAt < since) return [];
  const rows: ActivityRow[] = [];
  for (const step of message.steps ?? []) {
    // A child's own steps hang off the step that started it; the agent row says it.
    if (step.nestedUnder) continue;
    const label = clip(`${step.verb} ${step.target}`, 70);
    if (!label) continue;
    rows.push({
      id: step.id,
      kind: "step",
      label,
      state: step.status === "error" ? "error" : step.status === "done" ? "done" : "running",
      detail: cleanResult(step.result),
    });
  }
  for (const runId of message.subRunIds ?? []) {
    const run = subRuns[runId];
    if (!run) continue;
    rows.push({
      id: runId,
      kind: "agent",
      label: clip(`${run.agent}: ${run.task}`, 70),
      state:
        run.status === "done" ? "done" : run.status === "error" ? "error" : run.status === "stopped" ? "stopped" : "running",
    });
  }
  return rows;
}

/** One plain sentence for screen readers and the title of the list. */
export function activitySummary(rows: ActivityRow[]): string {
  const running = rows.filter((r) => r.state === "running").length;
  const failed = rows.filter((r) => r.state === "error").length;
  if (running) return running === 1 ? "Working on 1 thing" : `Working on ${running} things`;
  if (failed) return failed === 1 ? "1 thing did not work" : `${failed} things did not work`;
  return rows.length === 1 ? "Did 1 thing" : `Did ${rows.length} things`;
}
