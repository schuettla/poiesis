import { useEffect, useMemo, useRef, useState } from "react";
import type { Message } from "../../lib/types";
import * as api from "../../lib/api";
import type { VerifyClaim } from "../../lib/api";
import { experimentOn } from "../../lib/experiments";
import { isPersistedId, useAppStore } from "../../lib/store";
import {
  FINDING_WORDS,
  VIA_WORDS,
  answeredFromKnowledge,
  howWords,
  marksOf,
  needsRevising,
  progressText,
  sourceLine,
  summaryLine,
  trustKeyOf,
  trustSegments,
  type TrustKey,
} from "../../lib/verify";
import { ChevronIcon } from "../Icons/Icons";
import Toggle from "../Toggle/Toggle";
import RunText from "./RunText";
import "./Verify.css";

/** The check or cross the trust line's words used to start with. The dot beside the words
 * says it now, drawn like the rest of the app's status marks, so the glyph is dropped. */
const LEADING_GLYPH = /^[✓✕]\s*/;

/** How long a backed sentence is drawn as the check passes over it (`SEE-UI-1`). */
const SWEEP_MS = 1500;

/** `USE-UI-7`, `SEE-UI-1`: an answer with its check. The text carries the underlines, and the
 * panel under it carries the progress, the trust line, the claim list and the card of the
 * claim that was picked. Nothing here edits the answer. */
export default function CheckedAnswer({ message }: { message: Message }) {
  const on = useAppStore((s) => experimentOn(s, "fast-slow"));
  const convId = useAppStore((s) => s.activeConversationId);
  const entry = useAppStore((s) => s.verifications[message.id]);
  const loadVerifications = useAppStore((s) => s.loadVerifications);
  const [picked, setPicked] = useState<string | null>(null);
  const [showSupported, setShowSupported] = useState(false);
  const [, redraw] = useState(0);

  const persisted = isPersistedId(message.id);
  useEffect(() => {
    if (on && persisted && convId) void loadVerifications(convId);
  }, [on, persisted, convId, loadVerifications]);

  const check = on ? entry?.check ?? null : null;

  // A backed sentence is drawn for a moment as its check arrives, and then settles back to
  // plain text. Only a check that arrives live sweeps: one read from storage is already settled.
  const firstSeen = useRef(new Map<string, number>());
  const live = entry?.running === true && entry.progress?.phase === "auto";
  for (const c of check?.claims ?? []) {
    if (c.finding === "supported" && !firstSeen.current.has(c.id)) firstSeen.current.set(c.id, live ? Date.now() : 0);
  }
  const now = Date.now();
  const sweeping = new Set<string>();
  let soonest = Infinity;
  for (const [id, at] of firstSeen.current) {
    if (at > 0 && now - at < SWEEP_MS) {
      sweeping.add(id);
      soonest = Math.min(soonest, at + SWEEP_MS - now);
    }
  }
  const sweepKey = [...sweeping].join(",");
  useEffect(() => {
    if (!Number.isFinite(soonest)) return;
    const t = setTimeout(() => redraw((n) => n + 1), soonest + 30);
    return () => clearTimeout(t);
    // `soonest` follows from the set of sweeping sentences.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sweepKey]);

  const marks = useMemo(
    () => (check ? marksOf(check.claims, showSupported, sweeping) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [check, showSupported, sweepKey],
  );
  const claims = useMemo(
    () => ({
      marks,
      picked,
      onPick: (id: string) => setPicked((p) => (p === id ? null : id)),
    }),
    [marks, picked],
  );

  return (
    <>
      <RunText text={message.text} streaming={message.streaming} claims={marks.length > 0 ? claims : undefined} />
      {on && entry && (
        <VerifyPanel
          messageId={message.id}
          picked={picked}
          onPick={claims.onPick}
          showSupported={showSupported}
          onShowSupported={setShowSupported}
        />
      )}
    </>
  );
}

function VerifyPanel({
  messageId,
  picked,
  onPick,
  showSupported,
  onShowSupported,
}: {
  messageId: string;
  picked: string | null;
  onPick: (id: string) => void;
  showSupported: boolean;
  onShowSupported: (on: boolean) => void;
}) {
  const entry = useAppStore((s) => s.verifications[messageId]);
  const busy = useAppStore((s) => s.busy);
  const cancelVerify = useAppStore((s) => s.cancelVerify);
  const verifyMessage = useAppStore((s) => s.verifyMessage);
  const reviseFromVerification = useAppStore((s) => s.reviseFromVerification);
  const [reviseError, setReviseError] = useState<string | null>(null);
  const [filter, setFilter] = useState<TrustKey | null>(null);
  const [listOpen, setListOpen] = useState(false);
  const web = useWebSearchOn(entry?.check ? answeredFromKnowledge(entry.check) : false);
  if (!entry) return null;

  const auto = entry.check?.mode === "auto" || entry.progress?.phase === "auto";

  if (entry.running) {
    return (
      <div className="verify-row trust-line" role="status">
        <span className="verify-pulse" aria-hidden="true" />
        <span className="verify-progress">{progressText(entry.progress)}</span>
        {!auto && (
          <button type="button" className="verify-link" onClick={cancelVerify}>
            Cancel
          </button>
        )}
      </div>
    );
  }

  const check = entry.check;
  const claim = check?.claims.find((c) => c.id === picked) ?? null;
  const supportedCount = check ? check.claims.filter((c) => c.finding === "supported").length : 0;
  const segments = check && auto ? trustSegments(check.claims) : [];
  const shown = check ? check.claims.filter((c) => filter === null || trustKeyOf(c) === filter) : [];
  const knowledge = !!check && auto && answeredFromKnowledge(check);

  const pickSegment = (key: TrustKey) => {
    setFilter((f) => (f === key ? null : key));
    setListOpen(true);
  };

  return (
    <div className="verify-panel">
      {entry.error && (
        <p className="verify-error" role="alert">
          {entry.error}
        </p>
      )}
      {check && (
        <>
          <div className={`verify-row${auto ? " trust-line" : ""}`}>
            {auto ? (
              knowledge ? (
                <>
                  <span className="verify-summary">Answered from what I know · nothing checked</span>
                  {web && (
                    <button
                      type="button"
                      className="btn-secondary verify-btn"
                      disabled={busy}
                      onClick={() => void verifyMessage(messageId)}
                      title="Search the web for this answer's claims. Their words leave this computer."
                    >
                      Check against the web
                    </button>
                  )}
                </>
              ) : (
                segments.map((s, i) => (
                  <span key={s.key} className="trust-part">
                    {i > 0 && <span className="trust-dot"> · </span>}
                    <button
                      type="button"
                      className={`trust-seg vf-${s.key}`}
                      aria-pressed={filter === s.key}
                      onClick={() => pickSegment(s.key)}
                    >
                      <span className="verify-dot" aria-hidden="true" />
                      {s.text.replace(LEADING_GLYPH, "")}
                    </button>
                  </span>
                ))
              )
            ) : (
              <span className="verify-summary">{summaryLine(check.claims)}</span>
            )}
            {auto && check.summary && check.summary.over > 0 && (
              <span className="verify-aside">first {check.summary.sentences} sentences checked</span>
            )}
            {!auto && check.skipped > 0 && (
              <span className="verify-aside">
                {check.skipped} opinion{check.skipped === 1 ? "" : "s"} left out
              </span>
            )}
            {(supportedCount > 0 || needsRevising(check.claims)) && (
              <span className="verify-actions">
                {supportedCount > 0 && (
                  <label className="verify-toggle">
                    <Toggle checked={showSupported} onChange={onShowSupported} />
                    Underline supported
                  </label>
                )}
                {needsRevising(check.claims) && (
                  <button
                    type="button"
                    className="btn-secondary verify-btn"
                    disabled={busy}
                    onClick={() => {
                      setReviseError(null);
                      reviseFromVerification(messageId).catch((e) =>
                        setReviseError(e instanceof Error ? e.message : String(e)),
                      );
                    }}
                    title="Ask me to write a new answer from what the check found. This answer stays as it is."
                  >
                    Revise
                  </button>
                )}
              </span>
            )}
          </div>
          {reviseError && (
            <p className="verify-error" role="alert">
              {reviseError}
            </p>
          )}
          {check.note && <p className="verify-note">{check.note}</p>}
          {check.claims.length > 0 && (
            <details
              className="verify-claims"
              open={auto ? listOpen : undefined}
              onToggle={auto ? (e) => setListOpen((e.currentTarget as HTMLDetailsElement).open) : undefined}
            >
              <summary>
                {filter === null
                  ? `${check.claims.length} claim${check.claims.length === 1 ? "" : "s"}`
                  : `${shown.length} of ${check.claims.length} claims`}
                <span className="verify-twisty" aria-hidden="true">
                  <ChevronIcon dir="right" size={11} strokeWidth={1.6} />
                </span>
              </summary>
              <ul>
                {shown.map((c) => (
                  <li key={c.id}>
                    <button
                      type="button"
                      className={`verify-claim vf-${c.finding}${c.id === picked ? " picked" : ""}`}
                      aria-pressed={c.id === picked}
                      onClick={() => onPick(c.id)}
                    >
                      <span className="verify-finding">{FINDING_WORDS[c.finding]}</span>
                      <span className="verify-claim-text">{c.text}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </details>
          )}
          {claim && <ClaimCard messageId={messageId} claim={claim} />}
        </>
      )}
    </div>
  );
}

/** Whether the Web search tool is on, read when it matters: **Check against the web** is
 * offered only where that search can run. */
function useWebSearchOn(needed: boolean): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    if (!needed || !api.inTauri()) return;
    let live = true;
    api
      .listToolsets()
      .then((all) => live && setOn(all.some((t) => t.id === "web_search" && t.enabled)))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [needed]);
  return on;
}

function ClaimCard({ messageId, claim }: { messageId: string; claim: VerifyClaim }) {
  const markClaim = useAppStore((s) => s.markClaim);
  const reviewClaim = useAppStore((s) => s.reviewClaim);
  const [error, setError] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const mark = (value: "right" | "wrong") => {
    setError(null);
    markClaim(messageId, claim.id, value).catch(() => setError("I could not save that."));
  };
  // Anything a person can doubt that the slower model has not read yet.
  const canReview = claim.how !== "reviewed" && claim.how !== "ledger" && claim.passages.length > 0;
  return (
    <section className="verify-card" aria-label="Claim">
      <p className="verify-card-head">
        <span className={`verify-finding vf-${claim.finding}`}>{FINDING_WORDS[claim.finding]}</span>
        <span className="verify-how">{howWords(claim)}</span>
      </p>
      <p className="verify-card-claim">{claim.text}</p>
      {claim.reason && <p className="verify-card-reason">{claim.reason}</p>}
      {claim.passages.length === 0 ? (
        <p className="verify-card-none">{claim.how === "ledger" ? "No step of this run matches." : "No evidence was found."}</p>
      ) : (
        <ul className="verify-passages">
          {claim.passages.map((p, i) => (
            <li key={i}>
              <blockquote>{p.text}</blockquote>
              <p className="verify-source">
                {sourceLine(p)} · {VIA_WORDS[p.via]}
              </p>
            </li>
          ))}
        </ul>
      )}
      <div className="verify-card-actions">
        <div className="segmented verify-marks" role="group" aria-label="Your verdict on this claim">
          <button
            type="button"
            className="segmented-item"
            aria-pressed={claim.mark === "right"}
            onClick={() => mark("right")}
          >
            This is right
          </button>
          <button
            type="button"
            className="segmented-item"
            aria-pressed={claim.mark === "wrong"}
            onClick={() => mark("wrong")}
          >
            This is wrong
          </button>
        </div>
        {canReview && (
          <button
            type="button"
            className="btn-secondary verify-btn"
            disabled={reviewing}
            title="My slower model reads this sentence against the same passages and says what it finds."
            onClick={() => {
              setError(null);
              setReviewing(true);
              reviewClaim(messageId, claim.id)
                .catch((e) => setError(e instanceof Error ? e.message : String(e)))
                .finally(() => setReviewing(false));
            }}
          >
            {reviewing ? "Reviewing" : "Review this"}
          </button>
        )}
        {error && <span className="verify-error">{error}</span>}
      </div>
    </section>
  );
}
