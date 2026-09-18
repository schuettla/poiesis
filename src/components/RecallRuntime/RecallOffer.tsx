import { useEffect } from "react";
import { useAppStore } from "../../lib/store";
import "../Memory/Memory.css";

const DWELL_MS = 4000;

/**
 * SMP-2: the first-need prompt for the recall helper, asked once in the place
 * the user already is (a memory write, a folder attach) instead of behind a
 * settings tab nobody finds. Honest consent (SMP-2d): size and what it uses,
 * spelled out, never fetched without an explicit "Yes, fetch it".
 */
export default function RecallOffer() {
  const offer = useAppStore((s) => s.recallOffer);
  const accept = useAppStore((s) => s.acceptRecallOffer);
  const decline = useAppStore((s) => s.declineRecallOffer);

  useEffect(() => {
    if (offer?.stage !== "installed") return;
    const t = setTimeout(() => {
      useAppStore.setState((s) => (s.recallOffer?.stage === "installed" ? { recallOffer: null } : {}));
    }, DWELL_MS);
    return () => clearTimeout(t);
  }, [offer?.stage]);

  if (!offer) return null;

  if (offer.stage === "installed") {
    return (
      <div className="memory-toast recall-offer" role="status">
        <div className="memory-toast-line">
          <span className="memory-toast-mark" aria-hidden="true">
            ◆
          </span>
          <span className="memory-toast-text">I can search by meaning now.</span>
        </div>
      </div>
    );
  }

  if (offer.stage === "installing") {
    const pct = offer.progress?.total
      ? Math.round((offer.progress.received / offer.progress.total) * 100)
      : null;
    return (
      <div className="memory-toast recall-offer" role="status">
        <div className="memory-toast-line">
          <span className="memory-toast-mark" aria-hidden="true">
            ◆
          </span>
          <span className="memory-toast-text">
            Fetching the recall helper{pct !== null ? `… ${pct}%` : "…"}
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="memory-toast recall-offer" role="status">
      <div className="memory-toast-line">
        <span className="memory-toast-mark" aria-hidden="true">
          ◆
        </span>
        <span className="memory-toast-text">
          I can remember and search by meaning instead of just matching words. It needs a 130 MB
          helper that runs on your CPU.
        </span>
      </div>
      <div className="memory-toast-actions">
        <button className="btn-primary" onClick={() => accept()}>
          Yes, fetch it
        </button>
        <button className="memory-toast-undo" onClick={() => decline()}>
          Not now
        </button>
      </div>
    </div>
  );
}
