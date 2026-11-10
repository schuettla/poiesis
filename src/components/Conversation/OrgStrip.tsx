import type { ReactNode } from "react";
import * as api from "../../lib/api";
import { itemDot } from "../../lib/orgState";
import { useAppStore } from "../../lib/store";
import StyleGlyph from "../Mark/StyleGlyph";
import "./OrgStrip.css";

/**
 * `WRK-9`: the organization, as one line in the head of the plan card of an
 * answer that is worked item by item. A picture and not text, so the chat stays quiet: the preset, then
 * the items as dots (`○` waiting, `◐` in hand, `◔` being checked, `●` done, `↺` sent
 * back), and a mark for a helper out or a second look under way.
 *
 * **The arrangement follows the preset** (`WRK-12`, `WRK-14`), because how the work
 * is organised is the thing the strip shows:
 *
 * - Lean: one belt, one piece at a time.
 * - Command & Control: every dot passes through a node that is you.
 * - Mission Command: a gate before done. With no items (a goal), the criteria are
 *   the dots, and they pass the gate as they are met.
 * - Swarm: the dots side by side on a board, each with who has it.
 *
 * It is drawn from the plan the run is working to, which the backend sends whole
 * on every change, so it cannot show an item in a state it is not in. While the
 * run is going it is a live strip; once the run is over it settles into a static
 * record of the dots as they ended (a short contraction toward the header mark,
 * none under reduced motion). A click opens the Organization view on this run.
 *
 * There is none for Ad hoc, and none for a plan with nothing to draw: a short answer
 * looks as it always did.
 */
export default function OrgStrip({
  plan,
  live,
  fallback = null,
}: {
  plan: api.PlanView;
  live: boolean;
  /** What to show instead when there is no organization to draw. */
  fallback?: ReactNode;
}) {
  const preset = useAppStore((s) => (plan.style ? s.presets.find((p) => p.slug === plan.style) : undefined));
  const openOrganization = useAppStore((s) => s.openOrganization);
  const helpers = useAppStore((s) => {
    const conv = s.activeConversationId;
    return live && conv
      ? Object.values(s.subRuns).filter((r) => r.parentConversationId === conv && api.stillWorking(r.status)).length
      : 0;
  });
  if (!plan.style || plan.style === "ad-hoc") return <>{fallback}</>;

  // A custom preset or an offspring is drawn the way its parent is.
  const shape = preset?.glyph ?? plan.style;
  // A goal has criteria and no items: they are what passes the gate.
  const criteriaOnly = plan.items.length === 0 && shape === "mission-command" && !plan.awaiting_approval;
  const things = criteriaOnly
    ? (plan.criteria ?? []).map((c) => ({
        text: c.text,
        status: c.met ? "done" : "todo",
        glyph: c.met ? "●" : "○",
        word: c.met ? "met" : c.met === false ? "not met" : "not yet",
        by: undefined as string | null | undefined,
      }))
    : plan.items.map((item) => ({ text: item.text, status: item.status, ...itemDot(item), by: item.by }));
  if (things.length === 0) return <>{fallback}</>;

  const name = preset?.name ?? plan.style;
  const checking = plan.items.some((i) => i.review?.state === "checking");

  return (
    <button
      className={`org-strip ${live ? "live" : "settled"} shape-${shape}`}
      aria-label={`How I worked: ${name}`}
      title="Show how this chat is coordinated"
      onClick={() => openOrganization()}
    >
      <span className="org-name">
        <StyleGlyph glyph={shape} size={14} />
        {name}
      </span>
      <ol className="org-belt">
        {shape === "command-and-control" && (
          <li className="org-you" title="Every step passes through you">
            <span aria-hidden="true">◯</span>
            <span className="org-label">you</span>
          </li>
        )}
        {things.map((t, n) => (
          <li
            key={`${n}-${t.text}`}
            className={`org-item ${t.status}`}
            title={`${t.text}: ${t.word}${t.by ? `, ${t.by}` : ""}`}
          >
            <span className="org-dot" aria-label={`${t.text}: ${t.word}`} role="img">
              {t.glyph}
            </span>
            {t.by && <span className="org-by">{t.by}</span>}
          </li>
        ))}
        {shape === "mission-command" && (
          <li className="org-gate" title="A second look before the answer goes out">
            <span aria-hidden="true">◇</span>
            <span className="org-label">checked</span>
          </li>
        )}
      </ol>
      <span className="org-who">
        {helpers > 0 && <span title="A helper is out">◌ helper</span>}
        {checking && <span title="Someone is taking a second look">◌ second look</span>}
      </span>
    </button>
  );
}
