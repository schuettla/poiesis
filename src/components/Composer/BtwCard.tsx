import { useEffect } from "react";
import { useAppStore } from "../../lib/store";
import "./BtwCard.css";

/**
 * `BTW-UI-1`: a side question and its answer, above the box and outside the
 * conversation.
 *
 * It exists so a quick "wait, which file was that?" does not cost the run its
 * place or cost the transcript a detour. It is not in the chat unless you keep
 * it; Esc or Dismiss throws it away and stops the answer. While a local engine is
 * busy with the run, the question queues behind it and the card says so rather
 * than looking stuck.
 */
export default function BtwCard() {
  const side = useAppStore((s) => s.sideAnswer);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const keep = useAppStore((s) => s.keepSideAnswer);
  const dismiss = useAppStore((s) => s.dismissSideAnswer);
  const open = !!side && side.convId === activeConversationId;

  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") dismiss();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, dismiss]);

  if (!side || !open) return null;

  return (
    <section className="btw-card" aria-label="A question on the side" aria-live="polite">
      <p className="btw-card-question">{side.question}</p>
      {side.status === "waiting" && <p className="btw-card-wait">waiting for my engine…</p>}
      {side.status === "error" && (
        <p className="btw-card-error">I couldn't answer that: {side.error ?? "something went wrong"}.</p>
      )}
      {side.answer && <p className="btw-card-answer">{side.answer}</p>}
      <div className="btw-card-actions">
        {side.status === "done" && (
          <button className="btn-text" onClick={() => void keep()}>
            Keep in chat
          </button>
        )}
        <button className="btn-text" onClick={dismiss}>
          Dismiss
        </button>
      </div>
    </section>
  );
}
