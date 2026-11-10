import { groupByWeek, MAX_RINGS, FULL_WEEK } from "../../lib/growth";

/**
 * Concentric rings, one per week lived, darker where more was learned — a tree
 * ring, read from the inside out. Static and `aria-hidden`: the narrative
 * sentence beside it carries the same information in words.
 */
export default function GrowthRings({
  entries,
  born,
  changes = [],
}: {
  entries: { created: string }[];
  born: number;
  /** `WRK-21`: the second kind of entry. Every change to how I work is a small dot on the
   * ring of its week, next to what was learned. */
  changes?: { created: string }[];
}) {
  const all = groupByWeek(entries, born);
  const moved = groupByWeek(changes, born);
  // Older weeks merge into the innermost ring rather than shrinking forever.
  const visible = all.length <= MAX_RINGS ? all : all.slice(all.length - MAX_RINGS);
  const movedVisible = moved.length <= MAX_RINGS ? moved : moved.slice(moved.length - MAX_RINGS);
  if (all.length > MAX_RINGS) {
    visible[0] += all.slice(0, all.length - MAX_RINGS).reduce((a, b) => a + b, 0);
  }
  if (moved.length > MAX_RINGS) {
    movedVisible[0] += moved.slice(0, moved.length - MAX_RINGS).reduce((a, b) => a + b, 0);
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
      {/* Up to three dots a week, spread round the ring. */}
      {visible.map((_, i) => {
        const ring = 4 + step * i;
        const dots = Math.min(3, movedVisible[i + (movedVisible.length - visible.length)] ?? 0);
        return Array.from({ length: dots }, (_unused, d) => {
          const angle = (d * 2 * Math.PI) / 3 + i;
          return (
            <circle
              key={`c${i}-${d}`}
              className="growth-change"
              cx={36 + ring * Math.cos(angle)}
              cy={36 + ring * Math.sin(angle)}
              r="1.4"
              fill="currentColor"
              opacity="0.55"
            />
          );
        });
      })}
    </svg>
  );
}
