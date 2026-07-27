/** Growth rings (PRES-4): how much of the durable self was created in each
 * week Poiesis has been alive. Pure arithmetic, kept out of the component so
 * it can be checked without a DOM or a test framework. */

/** Weeks of growth the rings can show before older ones merge inward. */
export const MAX_RINGS = 12;
/** Entries in a week that make a ring fully dark. */
export const FULL_WEEK = 5;
const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * Count entries per week since `born`, oldest week first (PRES-4).
 *
 * Pure and unit-testable, and deliberately arithmetic rather than date-library:
 * a "week" here is a 7-day bucket from Poiesis's own birthday, which is what
 * the rings are actually about — not an ISO calendar week the user never
 * chose. Entries dated before `born` (hand-written files, restored trash) fall
 * into week 0 rather than being dropped.
 */
export function groupByWeek(entries: { created: string }[], born: number): number[] {
  const weeks = Math.max(1, Math.ceil((Date.now() - born) / WEEK_MS));
  const counts = new Array(weeks).fill(0);
  for (const e of entries) {
    const at = Date.parse(e.created);
    if (Number.isNaN(at)) continue;
    const idx = Math.floor((at - born) / WEEK_MS);
    counts[Math.min(weeks - 1, Math.max(0, idx))] += 1;
  }
  return counts;
}

