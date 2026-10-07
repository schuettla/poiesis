import { useEffect, useRef, useState } from "react";
import type { PendingQuestion } from "../../lib/types";
import { useAppStore } from "../../lib/store";
import "./QuestionCard.css";

/**
 * `AGC-3`: the run is paused on a real fork and asks you to decide.
 *
 * Two to four options, each a button (keys 1 to 4), and always a way to write
 * your own answer: typing in the composer answers it too, because a question
 * that could only be answered with a mouse would be a worse composer than the
 * one that is already there. The card says what it is in my voice and nothing
 * else: no timer, no urgency colour. The run waits for as long as it takes.
 */
export default function QuestionCard({ q }: { q: PendingQuestion }) {
  const answer = useAppStore((s) => s.answerQuestion);
  const [picked, setPicked] = useState<string[]>([]);
  const [other, setOther] = useState(false);
  const [text, setText] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  // A question is the thing to answer now. Focus the card, but never take the
  // cursor from something the user is already typing into.
  useEffect(() => {
    const active = document.activeElement;
    if (!active || active === document.body) ref.current?.focus();
  }, [q.id]);

  function choose(label: string) {
    if (!q.multi) {
      void answer({ choices: [label] });
      return;
    }
    setPicked((p) => (p.includes(label) ? p.filter((x) => x !== label) : [...p, label]));
  }

  function send() {
    const words = text.trim();
    if (!picked.length && !words) return;
    void answer({ choices: picked, text: words || undefined });
  }

  // Keys 1 to 4, when nothing editable has focus. The composer keeps its digits.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const n = Number(e.key);
      if (n >= 1 && n <= q.options.length) {
        e.preventDefault();
        choose(q.options[n - 1].label);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q.id, q.multi, q.options]);

  return (
    <div className="question-card" ref={ref} tabIndex={-1} role="group" aria-label="I need you to decide">
      <p className="question-card-head">I need you to decide</p>
      <p className="question-card-text">{q.question}</p>
      <div className="question-card-options">
        {q.options.map((o, i) => {
          const on = picked.includes(o.label);
          return (
            <button
              key={o.label}
              className={`question-card-option ${on ? "on" : ""}`}
              aria-pressed={q.multi ? on : undefined}
              onClick={() => choose(o.label)}
            >
              <span className="question-card-key" aria-hidden="true">
                {i + 1}
              </span>
              <span className="question-card-label">{o.label}</span>
              {o.detail && <span className="question-card-detail">{o.detail}</span>}
            </button>
          );
        })}
      </div>
      <div className="question-card-foot">
        {other ? (
          <input
            className="question-card-input"
            autoFocus
            value={text}
            placeholder="Or tell me in your own words"
            aria-label="Your own answer"
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                send();
              }
            }}
          />
        ) : (
          <button className="btn-text" onClick={() => setOther(true)}>
            Something else…
          </button>
        )}
        {(q.multi || other) && (
          <button className="btn-text" disabled={!picked.length && !text.trim()} onClick={send}>
            Send
          </button>
        )}
      </div>
    </div>
  );
}
