import { useAppStore } from "../../lib/store";

/**
 * A change the agent would like to make to itself, shown where it was raised
 * (SOUL-UI-2). Inline and quiet — the same visual register as a permission
 * request, never a modal. Nothing has changed when this appears; `Review`
 * takes the user to the place where accepting is an explicit act.
 */
export default function ProposalCard({ id }: { id: string }) {
  const proposal = useAppStore((s) => s.changeProposals.find((p) => p.id === id));
  const resolve = useAppStore((s) => s.resolveChangeProposal);
  const setView = useAppStore((s) => s.setView);

  // Answered elsewhere (or already resolved): the card is done.
  if (!proposal) return null;

  const lead =
    proposal.target === "recipe"
      ? `Poiesis wants to keep a procedure: ${proposal.slug ?? "untitled"}`
      : "Poiesis suggests a standing instruction";

  function review() {
    setView("settings");
    // The section renders on the next frame; two frames is enough to be safe.
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        document
          .getElementById("settings-personas")
          ?.scrollIntoView({ behavior: "smooth", block: "start" })
      )
    );
  }

  return (
    <div className="proposal-card">
      <p className="proposal-text">
        {lead} — {proposal.rationale}
      </p>
      <div className="proposal-actions">
        <button className="btn-text" onClick={review}>
          Review
        </button>
        <button className="btn-text" onClick={() => resolve(id, false)}>
          Dismiss
        </button>
      </div>
    </div>
  );
}
