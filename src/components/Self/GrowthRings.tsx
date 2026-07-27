import { groupByWeek, MAX_RINGS, FULL_WEEK } from "../../lib/growth";

/**
 * Concentric rings, one per week lived, darker where more was learned — a tree
 * ring, read from the inside out. Static and `aria-hidden`: the narrative
 * sentence beside it carries the same information in words.
 */
export default function GrowthRings({
  entries,
  born,
}: {
  entries: { created: string }[];
  born: number;
}) {
  const all = groupByWeek(entries, born);
  // Older weeks merge into the innermost ring rather than shrinking forever.
  const visible = all.length <= MAX_RINGS ? all : all.slice(all.length - MAX_RINGS);
  if (all.length > MAX_RINGS) {
    visible[0] += all.slice(0, all.length - MAX_RINGS).reduce((a, b) => a + b, 0);
  }

  const step = 32 / Math.max(visible.length, 1);
  return (
    <svg
      className="growth-rings"
      width="72"
      height="72"
      viewBox="0 0 72 72"
      aria-hidden="true"
    >
      <title>
        {all.length} weeks of growth — stronger rings are weeks I learned more
      </title>
      {visible.map((count, i) => (
        <circle
          key={i}
          cx="36"
          cy="36"
          r={4 + step * i}
          fill="none"
          stroke="currentColor"
          strokeWidth="1"
          opacity={0.15 + 0.45 * Math.min(1, count / FULL_WEEK)}
        />
      ))}
    </svg>
  );
}
