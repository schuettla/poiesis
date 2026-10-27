import type { LastTime } from "../../lib/api";
import { lastTimeLine } from "../../lib/lastTime";

/** `EPI-UI-1`: "Last time", in the slot of the composer's hint (`SEE-3`). One line,
 * truncated with the whole of it on the hover. Two ways out, neither of which sends. */
export default function LastTimeLine({
  offer,
  onTake,
  onDismiss,
}: {
  offer: LastTime | null;
  onTake: () => void;
  onDismiss: () => void;
}) {
  if (!offer) return null;
  const line = lastTimeLine(offer);
  return (
    <div className="composer-hint" role="status" title={line}>
      <span className="composer-hint-arrow" aria-hidden="true">
        →
      </span>
      <span className="composer-hint-text">{line}</span>
      <button type="button" className="composer-hint-action" onClick={onTake}>
        Do it the same way
      </button>
      <button type="button" className="composer-hint-action" onClick={onDismiss}>
        Start fresh
      </button>
    </div>
  );
}
