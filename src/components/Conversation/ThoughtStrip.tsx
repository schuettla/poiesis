import { useState } from "react";
import type { Message } from "../../lib/types";
import { decideStart } from "../../lib/api";
import { isPersistedId, useAppStore } from "../../lib/store";
import { engineLine, receiptOf, stripParts, type ReceiptRow } from "../../lib/thought";
import { BoltIcon, ChevronIcon } from "../Icons/Icons";

/**
 * `SEE-2`, state 1: what the quick decisions understood, above the run's first
 * text. It is on screen as soon as the turn's quick decisions are in, which
 * locally is tens of milliseconds, and it never spins: a decision that missed its
 * deadline says nothing. One line, so the turn never shifts by more than that.
 *
 * Only a group the person turned on themselves is ever named as left out, and
 * its click puts it back for this turn.
 */
export function ThoughtStrip({ message }: { message: Message }) {
  const convId = useAppStore((s) => s.activeConversationId);
  const restore = useAppStore((s) => s.restoreHeldTools);
  const t = message.thought;
  if (!t || !message.streaming) return null;
  const parts = stripParts(t.thought, t.back);
  const engine = engineLine(t.thought);
  if (parts.length === 0 && !engine) return null;
  return (
    <div className="thought-strip" role="status">
      <BoltIcon size={12} className="thought-bolt" />
      {parts.map((p, i) => (
        <span key={p.kind + i} className={`thought-part ${p.kind}`}>
          {i > 0 && <span className="thought-sep"> · </span>}
          {p.text}
          {p.action && convId && (
            <button type="button" className="thought-click" onClick={() => restore(convId, [p.action!.unit])}>
              {p.action.label}
            </button>
          )}
        </span>
      ))}
      {engine && <EngineOff start={engine.start} choose={engine.choose} text={engine.text} />}
    </div>
  );
}

/** The decision model could not be asked: said once for a conversation, never silent.
 * Start is only offered when a model is installed and simply not started; otherwise the
 * fix is choosing one, a cloud decision model included, on the Models page. */
function EngineOff({ text, start, choose }: { text: string; start: boolean; choose: boolean }) {
  const openModelsTab = useAppStore((s) => s.openModelsTab);
  const [state, setState] = useState<"idle" | "starting" | "on" | string>("idle");
  const go = () => {
    setState("starting");
    decideStart().then(
      () => setState("on"),
      (e) => setState(String(e?.message ?? e))
    );
  };
  if (state === "on") return <span className="thought-part">Quick thinking is on</span>;
  const failed = state !== "idle" && state !== "starting";
  return (
    <span className="thought-part engine">
      {text}
      {start && (
        <button type="button" className="thought-click" disabled={state === "starting"} onClick={go}>
          {state === "starting" ? "Starting" : "Start"}
        </button>
      )}
      {choose && (
        <button type="button" className="thought-click" onClick={() => openModelsTab("decide")}>
          Choose a decision model
        </button>
      )}
      {failed && <span className="thought-error"> {state}</span>}
    </span>
  );
}

/**
 * `SEE-2`, state 2: the same strip, moved to the foot of the run as one muted
 * line. Only measured facts are in it: the number of decisions from the log and
 * the size of what was kept out of the prompt. Opened, it lists the decisions that
 * acted or were shown, each with a way to say it was wrong.
 */
export function ThoughtReceipt({ message }: { message: Message }) {
  const convId = useAppStore((s) => s.activeConversationId);
  const markWrong = useAppStore((s) => s.markReceiptWrong);
  const reopen = useAppStore((s) => s.reopenPlanItem);
  // The sentences the answer checked itself on count toward the receipt (`SEE-1`).
  const check = useAppStore((s) => s.verifications[message.id]?.check ?? null);
  const t = message.thought;
  if (!t || message.streaming) return null;
  const receipt = receiptOf(t, message.plan, check);
  if (!receipt) return null;
  const canReopen = isPersistedId(message.id);
  return (
    <details className="thought-receipt">
      <summary>
        <BoltIcon size={12} className="thought-bolt" />
        <span className="thought-line">{receipt.line}</span>
        <span className="thought-twisty" aria-hidden="true">
          <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
        </span>
      </summary>
      <ul className="thought-rows">
        {receipt.rows.map((row) => (
          <li key={row.id} className="thought-row" title={row.hover}>
            <span className="thought-row-text">{row.text}</span>
            {!row.summary && (
              <RowActions
                row={row}
                reopen={canReopen && row.step !== null ? () => void reopen(message.id, row.step!) : undefined}
                onWrong={() => convId && void markWrong(convId, t.runId, row.id, row.units)}
              />
            )}
          </li>
        ))}
        {receipt.quiet > 0 && <li className="thought-row quiet">{receipt.quiet} more that changed nothing</li>}
      </ul>
    </details>
  );
}

function RowActions({ row, reopen, onWrong }: { row: ReceiptRow; reopen?: () => void; onWrong: () => void }) {
  return (
    <span className="thought-row-actions">
      {row.wrong ? (
        <span className="thought-marked">Marked wrong</span>
      ) : (
        <button type="button" className="thought-click" onClick={onWrong}>
          That was wrong
        </button>
      )}
      {row.wrong && reopen && (
        <button type="button" className="thought-click" onClick={reopen}>
          Reopen the step
        </button>
      )}
    </span>
  );
}
