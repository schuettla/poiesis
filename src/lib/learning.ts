import type { LearningReport } from "./api";

/** `MEV-UI-1`: the four lines of "Whether I'm learning" (Cognition plan, IV.7). */

/** 1100 -> "1.1k", 640 -> "640". */
export function compactChars(n: number): string {
  if (n < 1000) return String(n);
  const k = Math.round(n / 100) / 10;
  return `${Number.isInteger(k) ? k.toFixed(0) : k}k`;
}

const withLast = (now: string, last: string | null) => (last === null ? now : `${now} (last month ${last})`);

/** The lines to show, or `null` while there is under two weeks of history. */
export function learningLines(report: LearningReport | null): string[] | null {
  if (!report || !report.enough_history) return null;
  const { current, previous } = report;
  return [
    `Mistakes I repeated: ${withLast(String(current.repeat_mistakes), previous ? String(previous.repeat_mistakes) : null)}`,
    `Things you had to tell me twice: ${withLast(String(current.told_twice), previous ? String(previous.told_twice) : null)}`,
    `Memory I carry in every message: ${withLast(
      `${compactChars(current.always_on_chars)} characters`,
      previous ? compactChars(previous.always_on_chars) : null
    )}`,
    // Nothing measures a fix holding up until the improvement loop exists.
    "Fixes I made that held up: not yet",
  ];
}

/** What a check looks at, in words. The ids are the fixed set in `agent/habit.rs`. */
const CHECK_WORDS: Record<string, string> = {
  header_row: "the header row",
  path_exists: "that the file is there",
  file_nonempty: "that the file is not empty",
  json_parses: "that the JSON reads",
  row_count: "the row count",
};

/** `HAB-UI-1`: the Lessons tab line for a lesson that speaks up at a tool. */
export function firesLine(fires: { on: string; when?: string | null; check?: string | null }): string {
  const tool = fires.on.replace(/_/g, " ");
  const when = fires.when ? ` when it mentions ${fires.when}` : "";
  const check = fires.check && CHECK_WORDS[fires.check] ? `, and afterwards I look at ${CHECK_WORDS[fires.check]}` : "";
  return `I check this before ${tool}${when}${check}.`;
}

/** `HAB-3`: the plain sentence for a "check this before a tool" proposal, from its text
 * (`{"on": tool, "check": id or null}`). Anything unreadable gives `null`. */
export function habitLine(proposedText: string): string | null {
  try {
    const v = JSON.parse(proposedText) as { on?: unknown; check?: unknown };
    if (typeof v.on !== "string" || !v.on) return null;
    return firesLine({ on: v.on, check: typeof v.check === "string" ? v.check : null });
  } catch {
    return null;
  }
}
