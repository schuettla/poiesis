import { useState } from "react";
import * as api from "../../lib/api";
import type { SearchHit } from "../../lib/api";
import { useAppStore } from "../../lib/store";

type Pressed = null | { kind: "outdated"; now: string } | { kind: "not-relevant" };

/**
 * `CUE-UI-1`: one remembered or learned entry inside the recall chip, with why it came
 * and two ways to say it was wrong. Nothing here is a new row in the stream: the chip is
 * the existing one and this is its expanded detail.
 *
 * - **Outdated** (facts only) asks one line, "What is right now?", and the answer replaces
 *   the belief; the old one stays on the Memory tab as an earlier version.
 * - **Not relevant here** counts against the entry for this answer and changes nothing else.
 */
export default function RecallMemoryRow({
  hit,
  label,
  date,
}: {
  hit: SearchHit;
  label: string;
  date: string;
}) {
  const convId = useAppStore((s) => s.activeConversationId);
  const openChat = useAppStore((s) => s.setActiveConversation);
  const refreshMemoryContext = useAppStore((s) => s.refreshMemoryContext);
  const [asking, setAsking] = useState(false);
  const [text, setText] = useState("");
  const [pressed, setPressed] = useState<Pressed>(null);
  const [error, setError] = useState("");
  const isFact = hit.kind !== "lesson" && hit.kind !== "intention";
  // `INT-2`: a reminder fired once and is done; it has nothing to correct.
  const isReminder = hit.kind === "intention";

  async function outdate() {
    const now = text.trim();
    if (!now) return;
    try {
      await api.outdateMemoryFact(hit.title, now, convId);
      setPressed({ kind: "outdated", now });
      setAsking(false);
      setError("");
      void refreshMemoryContext();
    } catch (e) {
      setError(String(e));
    }
  }

  async function notRelevant() {
    try {
      await api.notRelevantMemory(hit.title, convId);
      setPressed({ kind: "not-relevant" });
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="recall-memory">
      <div className="recall-row">
        <span className="recall-chip memory">{label}</span>
        <span className="recall-title">{hit.title}</span>
        <span className="recall-date">{date}</span>
        <span className="recall-snippet">{hit.snippet}</span>
      </div>
      <div className="recall-why-line">
        {hit.why && <span className="recall-why">{hit.why}</span>}
        {hit.conversation_id && (
          <button className="recall-act" onClick={() => openChat(hit.conversation_id!)}>
            from that chat
          </button>
        )}
        {pressed === null && !asking && !isReminder && (
          <>
            {isFact && (
              <button className="recall-act" onClick={() => setAsking(true)}>
                Outdated
              </button>
            )}
            <button className="recall-act" onClick={() => void notRelevant()}>
              Not relevant here
            </button>
          </>
        )}
        {pressed?.kind === "outdated" && (
          <span className="recall-why">Replaced. Now: {pressed.now}</span>
        )}
        {pressed?.kind === "not-relevant" && (
          <span className="recall-why">Noted. I will use it less here.</span>
        )}
      </div>
      {asking && (
        <form
          className="recall-ask"
          onSubmit={(e) => {
            e.preventDefault();
            void outdate();
          }}
        >
          <label className="recall-ask-label">
            What is right now?
            <input
              className="recall-ask-input"
              value={text}
              autoFocus
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setAsking(false);
              }}
            />
          </label>
          <button type="submit" className="recall-act" disabled={!text.trim()}>
            Replace
          </button>
          <button type="button" className="recall-act" onClick={() => setAsking(false)}>
            Cancel
          </button>
        </form>
      )}
      {error && <span className="recall-why recall-error">{error}</span>}
    </div>
  );
}
