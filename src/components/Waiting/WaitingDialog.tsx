import { useEffect, useMemo, useState } from "react";
import type { ChangeProposal } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "./Waiting.css";

/** The words that name who is waiting. A member's role, or the one who leads. */
function whoWaits(p: ChangeProposal): string {
  return p.author?.trim() || "The run";
}

/** One thing a run left for the person. A change asks yes or not now; a question asks for words. */
function Entry({ p }: { p: ChangeProposal }) {
  const answer = useAppStore((s) => s.answerWaiting);
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const ask = p.target === "waiting-ask";

  async function go(accept: boolean) {
    setError("");
    setBusy(true);
    try {
      await answer(p.id, accept, ask ? text : undefined);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  }

  return (
    <li className="waiting-entry">
      <p className="waiting-line">
        {ask ? p.rationale : `${whoWaits(p)} would like to: ${p.rationale}`}
      </p>
      {ask && (
        <textarea
          className="waiting-answer"
          aria-label={`Your answer to ${whoWaits(p)}`}
          placeholder="Your answer"
          rows={2}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      )}
      {/* One line, always there, so the entry never changes height when an error comes. */}
      <p className="waiting-error" role="alert">
        {error}
      </p>
      <div className="waiting-actions">
        <button className="btn-text" disabled={busy || (ask && !text.trim())} onClick={() => void go(true)}>
          {ask ? "Send answer" : "Do it"}
        </button>
        <button className="btn-text" disabled={busy} onClick={() => void go(false)}>
          Not now
        </button>
      </div>
    </li>
  );
}

/**
 * `TEAM-19`: everything runs nobody watched left for the person, grouped by session. A dialog of a
 * fixed size: it scrolls inside, and an answered entry leaves the list without the dialog changing
 * height. When the last entry of a session is answered, that session goes on with the answers.
 */
export default function WaitingDialog() {
  const waiting = useAppStore((s) => s.waiting);
  const conversations = useAppStore((s) => s.conversations);
  const setOpen = useAppStore((s) => s.setWaitingOpen);
  const openSession = useAppStore((s) => s.openSession);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [setOpen]);

  const groups = useMemo(() => {
    const by = new Map<string, ChangeProposal[]>();
    for (const p of waiting) {
      const key = p.session_id ?? "";
      by.set(key, [...(by.get(key) ?? []), p]);
    }
    return [...by.entries()];
  }, [waiting]);

  return (
    <div className="confirm-backdrop" onClick={() => setOpen(false)}>
      <div
        className="confirm-card waiting-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Waiting for you"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="confirm-title">Waiting for you</h2>
        <p className="confirm-body">
          I did not make these changes and did not wait for an answer. When you have answered everything in a chat, I go on in it.
        </p>
        <div className="waiting-scroll">
          {groups.length === 0 && <p className="waiting-empty">Nothing is waiting.</p>}
          {groups.map(([session, entries]) => {
            const title = conversations.find((c) => c.id === session)?.title ?? "A chat that is gone";
            return (
              <section className="waiting-group" key={session} aria-label={title}>
                <div className="waiting-group-head">
                  <h3 className="waiting-title">{title}</h3>
                  <button
                    className="btn-text"
                    onClick={() => {
                      setOpen(false);
                      void openSession(session);
                    }}
                  >
                    Open
                  </button>
                </div>
                <ul className="waiting-list">
                  {entries.map((p) => (
                    <Entry key={p.id} p={p} />
                  ))}
                </ul>
              </section>
            );
          })}
        </div>
        <div className="confirm-actions">
          <button className="confirm-cancel" onClick={() => setOpen(false)}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
