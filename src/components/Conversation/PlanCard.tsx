import { useState } from "react";
import type { PlanItem, PlanView } from "../../lib/api";

/**
 * `PLN-UI-1`: the work a run means to do, above the steps that carry it out.
 *
 * It sits in the message column rather than in a side panel on purpose. The
 * plan and the timeline are the same story told at two altitudes — what I meant
 * to do, and what I did — and reading them top to bottom is what makes a long
 * run legible. Split across two surfaces, you have to hold one in your head
 * while looking at the other.
 *
 * Everything here follows from one rule: **the card shows the plan the run is
 * actually working against.** It is rendered from the plan the backend last
 * sent, which is the same object the model is shown at the top of every turn —
 * so it cannot drift into showing an intention the run abandoned. That is the
 * failure this whole feature exists to avoid, and it is the failure `step 1 of
 * 12` was: a status line that described something other than the work.
 */
export default function PlanCard({ plan }: { plan: PlanView }) {
  const [showEarlier, setShowEarlier] = useState(false);
  if (!plan.items.length) return null;

  const done = plan.items.filter((i) => i.status === "done").length;
  const dropped = plan.items.filter((i) => i.status === "dropped").length;
  // Dropped work is not outstanding, so it does not count against the total —
  // a plan reading "2 of 5" when three were deliberately dropped would look
  // like an abandoned run rather than a finished one.
  const counted = plan.items.length - dropped;

  return (
    <section className="plan-card" aria-label="My plan for this turn">
      <header className="plan-head">
        <span className="plan-title">My plan</span>
        <span className="plan-progress">
          {done} of {counted} done
          {dropped > 0 && `, ${dropped} dropped`}
        </span>
        {/* `PLN-UI-3`: a plan that changed says so. A plan that changed
            silently is untrustworthy in exactly the way this card exists to
            avoid — you would be reading a list with no way to know it had been
            a different list a minute ago. */}
        {plan.revisions > 0 && (
          <span className="plan-revised">
            revised, {ordinal(plan.revisions + 1)} version
          </span>
        )}
      </header>
      <ul className="plan-items">
        {plan.items.map((item, n) => (
          <PlanRow key={`${n}-${item.text}`} item={item} />
        ))}
      </ul>
      {plan.previous && plan.previous.length > 0 && (
        <details
          className="plan-history"
          open={showEarlier}
          onToggle={(e) => setShowEarlier(e.currentTarget.open)}
        >
          <summary>
            {plan.previous.length === 1
              ? "what I planned before"
              : `${plan.previous.length} earlier versions`}
          </summary>
          {plan.previous
            .slice()
            .reverse()
            .map((version, n) => (
              <ol className="plan-past" key={n}>
                {version.map((text, i) => (
                  <li key={`${i}-${text}`}>{text}</li>
                ))}
              </ol>
            ))}
        </details>
      )}
    </section>
  );
}

/**
 * One line of the checklist. Two columns, not a wrapping row: the box holds its
 * own column and the text wraps inside the second, so a long item hangs under
 * itself instead of flowing back under the box and breaking the list apart.
 *
 * The status rides on the box as its label rather than as hidden text beside
 * it. Hidden text is announced correctly but it also lands in the clipboard, so
 * copying a plan produced "doing:Research the market" — the box carries the
 * meaning for a screen reader and copies as a plain checkbox for everyone else.
 */
function PlanRow({ item }: { item: PlanItem }) {
  return (
    <li className={`plan-item ${item.status}`}>
      <span className="plan-mark" role="img" aria-label={`${item.status}:`}>
        {mark(item.status)}
      </span>
      <span className="plan-body">
        <span className="plan-text">{item.text}</span>
        {/* A dropped item keeps its reason beside it, because the reason is the
            only thing that makes the drop honest rather than a disappearance. */}
        {item.status === "dropped" && item.why && (
          <span className="plan-why"> — {item.why}</span>
        )}
        {item.added && <span className="plan-added"> added later</span>}
      </span>
    </li>
  );
}

/** A checkbox, because that is what this is. All four marks come from the same
 * family so they line up in one column and read as four states of one thing;
 * the half-filled box is work in progress. Shape, not colour: the status has to
 * survive being printed, screenshotted, and read by someone who cannot tell
 * green from grey. */
function mark(status: PlanItem["status"]): string {
  switch (status) {
    case "done":
      return "☑";
    case "doing":
      return "◧";
    case "dropped":
      return "☒";
    default:
      return "☐";
  }
}

function ordinal(n: number): string {
  const names = ["", "1st", "2nd", "3rd"];
  return names[n] ?? `${n}th`;
}
