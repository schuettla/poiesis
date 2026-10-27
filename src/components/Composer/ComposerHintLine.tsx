import type { ComposerHint } from "../../lib/api";

/** `SEE-3`: the one line the composer may show beside the send button. One line, truncated
 * with the whole of it on the hover; the box never changes height because of it. */
export default function ComposerHintLine({
  hint,
  onTake,
  onDismiss,
}: {
  hint: ComposerHint | null;
  onTake: () => void;
  onDismiss: () => void;
}) {
  if (!hint) return null;
  return (
    <div className="composer-hint" role="status" title={hint.title}>
      <span className="composer-hint-arrow" aria-hidden="true">
        →
      </span>
      <span className="composer-hint-text">{hint.text}</span>
      <button type="button" className="composer-hint-action" onClick={onTake}>
        {hint.action}
      </button>
      <button type="button" className="composer-hint-x" aria-label="Dismiss this hint" onClick={onDismiss}>
        ×
      </button>
    </div>
  );
}
