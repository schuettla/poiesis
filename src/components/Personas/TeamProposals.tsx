import { useMemo, useState } from "react";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { teamProposals } from "../../lib/teamProposals";

const say = (e: unknown) => String(e).replace(/^Error:\s*/, "");

/**
 * `TEAM-23`: what Poiesis would like to change about this team, waiting for a yes or a no. Each is a
 * plain sentence and a reason; Yes makes the change as a new version of the team (the History shows it
 * was Poiesis who made it, and can go back), No is remembered, so the same thing is not asked again.
 * While something typed here is not saved, the answer waits: a change made now would take it away.
 */
export default function TeamProposals({ slug, unsaved, onChanged }: { slug: string; unsaved: boolean; onChanged: () => Promise<void> }) {
  const all = useAppStore((s) => s.changeProposals);
  const refresh = useAppStore((s) => s.refreshChangeProposals);
  const mine = useMemo(() => teamProposals(all).filter((p) => p.team === slug), [all, slug]);
  const [busy, setBusy] = useState<string | null>(null);
  const [problem, setProblem] = useState<{ id: string; text: string } | null>(null);
  if (mine.length === 0) return null;

  async function answer(id: string, what: "yes" | "no") {
    setBusy(id);
    setProblem(null);
    try {
      await api.wayProposalAnswer(id, what);
      await refresh();
      await onChanged();
    } catch (e) {
      setProblem({ id, text: say(e) });
      await refresh();
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="team-proposals" aria-label="Waiting for you">
      {mine.map((p) => (
        <div className="team-proposal" key={p.id} role="group" aria-label={p.lead}>
          <p className="team-proposal-lead">{p.lead}</p>
          <p className="team-proposal-quote">{p.quote}</p>
          <p className="field-hint">Why: {p.why}</p>
          <div className="team-proposal-actions">
            <button className="btn-primary" disabled={busy !== null || unsaved} onClick={() => void answer(p.id, "yes")}>
              Yes
            </button>
            <button className="btn-text" disabled={busy !== null} onClick={() => void answer(p.id, "no")}>
              No
            </button>
            {unsaved && <span className="field-hint">Save or undo what you changed here first.</span>}
          </div>
          {problem?.id === p.id && (
            <p className="field-hint" role="alert">
              {problem.text}
            </p>
          )}
        </div>
      ))}
    </section>
  );
}
