import { useState } from "react";
import * as api from "../../lib/api";
import type { MoveNote, PlanItem, PlanView } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { ChevronIcon, KebabIcon } from "../Icons/Icons";
import BranchDialog from "./BranchDialog";
import OrgStrip from "./OrgStrip";

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
export default function PlanCard({
  plan,
  live = false,
  actionable = false,
  stopped,
  runId,
  branching,
  folded = false,
}: {
  plan: PlanView;
  /** The run is still going: the organization in the head is drawn live. */
  live?: boolean;
  actionable?: boolean;
  /** `WRK-1`: the check that stopped the line, while it is stopped. Only the
   * live card is given one. */
  stopped?: string;
  /** `WRK-7`: the run this card belongs to while it is still going: what makes
   * "Not this time" possible. Only the live card is given one. */
  runId?: string;
  /** `WRK-6`: the item being tried two ways right now. */
  branching?: number;
  /** A newer turn has a plan of its own: this one is one line until opened, so
   * the chat does not repeat the whole plan on every turn. */
  folded?: boolean;
}) {
  const [showEarlier, setShowEarlier] = useState(false);
  // Open or closed like the steps under it: the newest plan starts open, an
  // earlier one closed, and either is yours to turn.
  const [open, setOpen] = useState(!folded);
  const approve = useAppStore((s) => s.approvePlan);
  const revise = useAppStore((s) => s.revisePlan);
  // `WRK-2`: the way of working this plan ran under, in words. Ad hoc has none.
  const styleName = useAppStore((s) =>
    plan.style
      ? (s.presets.find((p) => p.slug === plan.style)?.name ?? null)
      : null,
  );
  const criteria = plan.criteria ?? [];
  // A plan of criteria alone, waiting for the yes, is still a card.
  if (!plan.items.length && !criteria.length) return null;
  // A move that was not about one item (starting fresh) is told in the head.
  const runLevel = (plan.moves ?? []).filter(
    (m) => m.item === undefined || m.item === null,
  );

  const done = plan.items.filter((i) => i.status === "done").length;
  const dropped = plan.items.filter((i) => i.status === "dropped").length;
  // Dropped work is not outstanding, so it does not count against the total —
  // a plan reading "2 of 5" when three were deliberately dropped would look
  // like an abandoned run rather than a finished one.
  const counted = plan.items.length - dropped;

  return (
    <section
      className={`plan-card${open ? " open" : ""}`}
      aria-label="My plan for this turn"
    >
      {/* `WRK-1`: while a check is failing, nothing else on this card matters
          more. Said once, at the top, and gone the moment it passes. */}
      {stopped && (
        <div className="plan-stopped" role="status">
          Stopped: <code>{stopped}</code> is failing. I'm fixing that before
          anything else.
        </div>
      )}
      <header className="plan-head">
        <button
          className="plan-toggle"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <span className="plan-twisty" aria-hidden="true">
            <ChevronIcon dir="right" size={12} strokeWidth={1.6} />
          </span>
          <span className="plan-title">My plan</span>
          {plan.items.length > 0 && (
            <span className="plan-progress">
              {done} of {counted} done
              {dropped > 0 && `, ${dropped} dropped`}
            </span>
          )}
        </button>
        {/* `PLN-UI-3`: a plan that changed says so. A plan that changed
            silently is untrustworthy in exactly the way this card exists to
            avoid — you would be reading a list with no way to know it had been
            a different list a minute ago. */}
        {plan.revisions > 0 && (
          <span className="plan-revised">
            revised, {ordinal(plan.revisions + 1)} version
          </span>
        )}
        {runLevel.map((m) => (
          <MoveInfo key={`${m.rule}-${m.state}`} move={m} runId={runId} />
        ))}
        {/* `WRK-9`: how I am working, as its glyph and the items as dots, in
            the head of the plan it organizes. Ad hoc has no organization to
            draw, and a plan with nothing to draw keeps the name alone. */}
        <OrgStrip
          plan={plan}
          live={live}
          fallback={styleName ? <span className="plan-style">{styleName}</span> : null}
        />
        {/* `WRK-17`: a run that is over can be kept as a preset. */}
        {plan.style && !runId && <KeepAsPreset plan={plan} />}
      </header>
      {open && (
        <>
          {/* `WRK-10`: what done means, agreed before anything changes. The boxes are ticked
          by the final check, which never saw how the work went; one that was not met stays
          open with its reason. */}
          {criteria.length > 0 && (
            <section className="plan-criteria" aria-label="What done means">
              <h4 className="plan-criteria-title">What done means</h4>
              <ul>
                {criteria.map((c, n) => (
                  <li
                    key={`${n}-${c.text}`}
                    className={`plan-criterion${c.met ? " met" : ""}`}
                  >
                    <span
                      className="plan-mark"
                      role="img"
                      aria-label={
                        c.met
                          ? "met:"
                          : c.met === false
                            ? "not met:"
                            : "not checked yet:"
                      }
                    >
                      {c.met ? "☑" : "☐"}
                    </span>
                    <span className="plan-body">
                      <span className="plan-text">{c.text}</span>
                      {c.met === false && c.why && (
                        <span className="plan-why"> — {c.why}</span>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}
          <ul className="plan-items">
            {plan.items.map((item, n) => (
              <PlanRow
                key={`${n}-${item.text}`}
                item={item}
                moves={(plan.moves ?? []).filter((m) => m.item === n)}
                runId={runId}
                trying={branching === n}
              />
            ))}
          </ul>
          {/* `PLF-4`: a plan I wrote while planning first. Nothing has changed yet, and
          nothing will until you say so. Only on the last turn and only while I am
          idle: approving an older plan would run work the conversation has since
          moved past. */}
          {plan.awaiting_approval && actionable && (
            <div
              className="plan-approval"
              role="group"
              aria-label="Approve my plan"
            >
              <span className="plan-approval-text">
                {criteria.length > 0
                  ? "This is what I'll check myself against. Nothing has changed yet."
                  : "This is my plan. Nothing has changed yet."}
              </span>
              <span className="plan-approval-actions">
                <button className="btn-secondary" onClick={revise}>
                  Change something
                </button>
                <button
                  className="btn-primary"
                  onClick={() => void approve(plan)}
                >
                  Go ahead
                </button>
              </span>
            </div>
          )}
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
        </>
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
function PlanRow({
  item,
  moves,
  runId,
  trying,
}: {
  item: PlanItem;
  moves: MoveNote[];
  runId?: string;
  trying: boolean;
}) {
  const [other, setOther] = useState(false);
  const review = item.review ?? null;
  // `WRK-4`: while someone else is looking at it, the item is neither doing nor
  // done; once sent back it is in hand again, with a mark of its own.
  const looking = review?.state === "checking";
  const back = review?.state === "sent_back" && item.status === "doing";
  const glyph = looking ? "◔" : back ? "↺" : mark(item.status);
  const label = looking ? "second look" : back ? "sent back" : item.status;
  return (
    <li
      className={`plan-item ${item.status}${looking ? " checking" : ""}${back ? " sent-back" : ""}`}
    >
      <span className="plan-mark" role="img" aria-label={`${label}:`}>
        {glyph}
      </span>
      <span className="plan-body">
        <span className="plan-text">{item.text}</span>
        {/* `WRK-14`: who has it, when it is not me. */}
        {item.by && <span className="plan-by"> — {item.by}</span>}
        {/* A dropped item keeps its reason beside it, because the reason is the
            only thing that makes the drop honest rather than a disappearance. */}
        {item.status === "dropped" && item.why && (
          <span className="plan-why"> — {item.why}</span>
        )}
        {item.added && <span className="plan-added"> added later</span>}
        {/* `WRK-2`: what done means for this item, so the final check has
            something to be held against. */}
        {item.done_when && (
          <span className="plan-done-when">Done when: {item.done_when}</span>
        )}
        {/* `WRK-4`: what the second look said. A verdict that sends an item back
            always has its reason; one without a reason does not exist. */}
        {review?.state === "checking" && (
          <span className="plan-review">Second look…</span>
        )}
        {review?.state === "checked" && (
          <span className="plan-review" title={review.reason ?? undefined}>
            Checked
          </span>
        )}
        {review?.state === "sent_back" && (
          <span className="plan-review back">
            Sent back: {review.reason ?? "it is not done yet"}
          </span>
        )}
        {/* `WRK-6`: two ways, while they are being tried and after one was kept. */}
        {trying && (
          <ul className="plan-ways" aria-label="Two ways">
            <li>
              <span aria-hidden="true">◧</span> Way A: the most direct fix
            </li>
            <li>
              <span aria-hidden="true">◧</span> Way B: a different route
            </li>
          </ul>
        )}
        {item.branches && !trying && (
          <span className="plan-kept">
            Kept {item.branches.kept.toUpperCase()}: {item.branches.reason}.{" "}
            <button
              className="btn-text plan-other"
              onClick={() => setOther(true)}
            >
              See the other way
            </button>
          </span>
        )}
        {moves.length > 0 && (
          <span className="plan-moves">
            {moves.map((m) => (
              <MoveInfo key={`${m.rule}-${m.state}`} move={m} runId={runId} />
            ))}
          </span>
        )}
      </span>
      {other && item.branches && (
        <BranchDialog
          id={item.branches.id}
          kept={item.branches.kept}
          onClose={() => setOther(false)}
        />
      )}
    </li>
  );
}

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** Where a rule comes from, in words: "Learned in October". */
function whereFrom(m: MoveNote): string {
  const month = m.since ? MONTHS[Number(m.since.slice(5, 7)) - 1] : undefined;
  switch (m.from) {
    case "always":
      return "Always";
    case "learned":
      return month ? `Learned in ${month}` : "Learned";
    case "trying":
      return "Trying it out";
    default:
      return "Part of how I'm working";
  }
}

/**
 * `WRK-7`: ⓘ on a move. Hover or focus says the rule behind it, in a sentence,
 * and where it comes from. While the run is going there is "Not this time", which
 * skips that move once; clicking it is also something the organization hears
 * about, because a rule that keeps being skipped is one to look at.
 */
function MoveInfo({ move, runId }: { move: MoveNote; runId?: string }) {
  const [said, setSaid] = useState<"none" | "queued" | "over">("none");
  const skip = async () => {
    if (!runId) return;
    try {
      setSaid((await api.skipMove(runId, move.rule)) ? "queued" : "over");
    } catch {
      setSaid("over");
    }
  };
  return (
    <span className="plan-move">
      <button className="plan-info" aria-label={`Why: ${move.text}`}>
        ⓘ
      </button>
      <span className="plan-move-card" role="tooltip">
        <span className="plan-move-rule">{move.text}</span>
        <span className="plan-move-from">{whereFrom(move)}</span>
        {move.state === "skipped" && (
          <span className="plan-move-from">Skipped this time.</span>
        )}
        {runId && move.state === "fired" && said === "none" && (
          <button
            className="btn-text plan-move-skip"
            onClick={() => void skip()}
          >
            Not this time
          </button>
        )}
        {said === "queued" && (
          <span className="plan-move-from">I'll skip it once.</span>
        )}
        {said === "over" && (
          <span className="plan-move-from">That run is over.</span>
        )}
      </span>
    </span>
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

/**
 * `WRK-17`: "Keep this as a style of my own". Makes a preset of your own from the
 * one this run worked under, with the moves that fired on top of it that did not come
 * from that preset (what was learned, what was being tried). It opens the same dialog
 * as every other way of making one, with those moves ready, and creates nothing until
 * you give it a name.
 */
function KeepAsPreset({ plan }: { plan: PlanView }) {
  const [open, setOpen] = useState(false);
  const openPresetDialog = useAppStore((s) => s.openPresetDialog);
  const extra: api.RuleRef[] = [];
  for (const m of plan.moves ?? []) {
    if (m.state !== "fired" || !m.when) continue;
    if (m.from !== "learned" && m.from !== "trying") continue;
    if (!extra.some((r) => r.when === m.when && r.move === m.move))
      extra.push({ when: m.when, move: m.move });
  }
  return (
    <span className="plan-more">
      <button
        className="plan-more-btn"
        aria-label="More about this style"
        aria-haspopup="menu"
        aria-expanded={open}
        title="More"
        onClick={() => setOpen((o) => !o)}
      >
        <KebabIcon size={14} />
      </button>
      {open && (
        <>
          <div className="row-menu-backdrop" onClick={() => setOpen(false)} />
          <div className="row-menu" role="menu">
            <button
              className="row-menu-item"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                openPresetDialog({
                  mode: "new",
                  from: plan.style ?? null,
                  add: extra,
                });
              }}
            >
              Keep this as a style of my own
            </button>
          </div>
        </>
      )}
    </span>
  );
}
