import type { CheckupLine } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "./CheckupCard.css";

/** What the state says, in words. Never green or red: a state has to survive
 * being read aloud, printed, and seen by someone who cannot tell the two apart. */
const STATE_WORD: Record<CheckupLine["state"], string> = {
  fine: "fine",
  needs_you: "needs you",
  off: "off",
};

/**
 * `CHK-UI-1`: myself, looked over, one sentence per area.
 *
 * The lines are mine and speak as I do. A line that has something to do carries
 * exactly one plain link to where you would do it. Nothing here is a gauge, a
 * percentage or a colour, because this page is about a self, not a server.
 */
export default function CheckupCard({ lines, summary, at }: { lines: CheckupLine[]; summary?: string; at: string }) {
  const openRuntime = useAppStore((s) => s.openRuntime);
  const setView = useAppStore((s) => s.setView);
  const openSelf = useAppStore((s) => s.openSelf);

  function go(target: NonNullable<CheckupLine["action"]>["target"]) {
    if (target === "runtime") openRuntime("chat");
    else if (target === "recall") openRuntime("recall");
    else if (target === "providers") setView("providers");
    else if (target === "connectors") setView("apps");
    else openSelf("health");
  }

  return (
    <section className="checkup-card" aria-label="How I am">
      <p className="checkup-card-head">
        {summary ?? "I checked myself."}
        <span className="checkup-card-time"> · {at}</span>
      </p>
      <ul className="checkup-card-lines">
        {lines.map((l, i) => (
          <li className={`checkup-card-line ${l.state}`} key={`${l.area}-${i}`}>
            <span className="checkup-card-state">{STATE_WORD[l.state]}</span>
            <span className="checkup-card-text">
              {l.text}
              {l.action && (
                <>
                  {" "}
                  <button className="checkup-card-link" onClick={() => go(l.action!.target)}>
                    {l.action.label}
                  </button>
                </>
              )}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
