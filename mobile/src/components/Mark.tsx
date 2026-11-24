import "./Mark.css";

/** The mark, as on the desktop: a membrane and a nucleus. Resting, working (it breathes), or away (dotted). */
export type MarkState = "resting" | "working" | "away";

const LABELS: Record<MarkState, string> = {
  resting: "resting",
  working: "working",
  away: "out of reach",
};

export default function Mark({ state = "resting", size = 28 }: { state?: MarkState; size?: number }) {
  const label = `Poiesis, ${LABELS[state]}`;
  return (
    <svg className={`mark state-${state}`} width={size} height={size} viewBox="0 0 24 24" role="img" aria-label={label}>
      <title>{label}</title>
      <circle className="mark-membrane" cx="12" cy="12" r="8" fill="none" strokeWidth="1" strokeDasharray={state === "away" ? "2 3" : undefined} />
      <circle className="mark-nucleus" cx="12" cy="12" r="3" />
    </svg>
  );
}
