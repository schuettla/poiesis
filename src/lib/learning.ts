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
