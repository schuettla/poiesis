import type { Approval } from "../types";

/** Longest piece of a diff or file shown on the card. The rest is on the desktop. */
const SHOWN = 600;

/** The plain question and two large buttons. Yes is once, never "always": that is for the desktop. */
export default function ApprovalCard({ card, onAnswer }: { card: Approval; onAnswer: (allow: boolean) => void }) {
  const detail = card.argv?.length ? card.argv.join(" ") : card.diff;
  const cut = detail && detail.length > SHOWN ? `${detail.slice(0, SHOWN)} ...` : detail;
  return (
    <section className="card approval" role="alertdialog" aria-label="Poiesis is asking">
      <p className="card-label">Poiesis is asking</p>
      <p className="approval-question">{card.summary}</p>
      {card.path && <p className="approval-path">{card.path}</p>}
      {cut && <pre className="approval-detail">{cut}</pre>}
      {detail && detail.length > SHOWN && <p className="approval-more">See the desktop for the rest.</p>}
      <div className="card-actions">
        <button className="btn btn-secondary" onClick={() => onAnswer(false)}>
          No
        </button>
        <button className="btn btn-primary" onClick={() => onAnswer(true)}>
          Allow once
        </button>
      </div>
    </section>
  );
}
