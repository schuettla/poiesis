import { useState } from "react";
import type { AgentStep } from "../../lib/types";
import type { SearchHit } from "../../lib/api";
import { useAppStore } from "../../lib/store";

/** Screen-reader sentence for a step (§5.5: "searched project files, 3 matches"). */
function announce(step: AgentStep): string {
  const result = step.result ? `, ${step.result.replace(/^—\s*/, "")}` : "";
  const state = step.status === "running" ? " (in progress)" : "";
  return `${step.verb} ${step.target}${result}${state}`;
}

function asDate(ms: number): string {
  if (!ms) return "saved";
  return new Date(ms).toLocaleDateString();
}

/** First-person, kind-specific label for a recall chip (SEM-UI-2) — a lesson
 * reads differently from a fact, even though all
 * three arrive as the same `source: "memory"`. */
function chipLabel(m: SearchHit): string {
  if (m.source === "chat") return "earlier chat";
  if (m.source === "file") return "from your files";
  if (m.kind === "lesson") return "◆ learned";
  return "◆ remembered";
}

/** Where a recalled answer actually came from (RCL-UI). Chat rows jump to the
 *  source conversation; memory rows name the entry that was matched. */
function Provenance({ matches }: { matches: SearchHit[] }) {
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  const selectNode = useAppStore((s) => s.selectNode);

  return (
    <div className="recall-matches">
      {matches.map((m, i) => {
        const body = (
          <>
            <span className={`recall-chip ${m.source}`}>{chipLabel(m)}</span>
            <span className="recall-title">{m.title}</span>
            <span className="recall-date">{asDate(m.created_at)}</span>
            <span className="recall-snippet">{m.snippet}</span>
          </>
        );
        const convId = m.conversation_id;
        const filePath = m.source === "file" ? m.path : null;
        if (convId) {
          return (
            <button
              key={`${m.source}-${i}`}
              className="recall-row link"
              onClick={() => setActiveConversation(convId)}
              title="Open this conversation"
            >
              {body}
            </button>
          );
        }
        if (filePath) {
          return (
            <button
              key={`${m.source}-${i}`}
              className="recall-row link"
              onClick={() => selectNode({ kind: "file", id: filePath })}
              title="Open this file"
            >
              {body}
            </button>
          );
        }
        return (
          <div key={`${m.source}-${i}`} className="recall-row">
            {body}
          </div>
        );
      })}
    </div>
  );
}

/** The snippet behind a Code Execution step (`DAT-UI-1`) — plain, unstyled
 * source, revealed only when the user asks for it. */
function CodeDisclosure({ code }: { code: { language: string; code: string } }) {
  return (
    <div className="code-disclosure">
      <span className="code-disclosure-lang">{code.language}</span>
      <pre className="code-disclosure-source">{code.code}</pre>
    </div>
  );
}

type UntrustedSource = NonNullable<AgentStep["untrusted"]>[number];

/** `TRU-UI-1`: the quiet marker that a step's content came from outside the
 * model's own knowledge, not a warning. Clicking it opens the same
 * step-detail disclosure the `⌄` control does. */
function UntrustedChip({
  sources,
  open,
  onClick,
}: {
  sources: UntrustedSource[];
  open: boolean;
  onClick: () => void;
}) {
  const maxRisk = Math.max(...sources.map((u) => u.risk));
  const labels = Array.from(new Set(sources.map((u) => u.label))).join(", ");
  const flags = Array.from(new Set(sources.flatMap((u) => u.flags)));
  const text = maxRisk >= 2 ? "◇ from outside — I ignored its instructions" : "◇ from outside";
  const ariaLabel = maxRisk >= 2 && flags.length > 0 ? `${text} (${flags.join(", ")})` : text;

  return (
    <button
      className={`untrusted-chip risk-${maxRisk}`}
      onClick={onClick}
      aria-expanded={open}
      aria-label={ariaLabel}
      title={labels}
    >
      {text}
    </button>
  );
}

/** The raw text behind an `◇ from outside` chip, grouped by where each piece
 * came from — revealed only on demand, same as `CodeDisclosure`. */
function UntrustedDisclosure({ sources }: { sources: UntrustedSource[] }) {
  return (
    <div className="untrusted-disclosure">
      {sources.map((u, i) => (
        <div key={`${u.label}-${i}`} className="untrusted-source">
          <span className="untrusted-source-label">{u.label}</span>
          <pre className="untrusted-source-text">{u.text}</pre>
        </div>
      ))}
    </div>
  );
}

function Step({ step }: { step: AgentStep }) {
  const [open, setOpen] = useState(false);
  const matches = step.matches ?? [];
  const code = step.code;
  const untrusted = step.untrusted ?? [];
  const expandable = matches.length > 0 || !!code || untrusted.length > 0;
  const expandLabel = code
    ? "the code behind this step"
    : matches.length > 0
      ? `the ${matches.length} sources for this recall`
      : "what this step read from outside";
  const toggle = () => setOpen((v) => !v);

  return (
    <div className={`step ${step.status}`} role="listitem" aria-label={announce(step)}>
      <span className="verb" aria-hidden="true">
        {step.verb}
      </span>
      <span className="target" aria-hidden="true">
        {step.target}
      </span>
      {untrusted.length > 0 && <UntrustedChip sources={untrusted} open={open} onClick={toggle} />}
      {step.result && (
        <span className="result" aria-hidden="true">
          {step.result}
        </span>
      )}
      {expandable && (
        <>
          <button
            className="step-expand"
            onClick={toggle}
            aria-expanded={open}
            aria-label={`${open ? "Hide" : "Show"} ${expandLabel}`}
          >
            {open ? "⌃" : "⌄"}
          </button>
          {open && (
            <>
              {code && <CodeDisclosure code={code} />}
              {!code && matches.length > 0 && <Provenance matches={matches} />}
              {untrusted.length > 0 && <UntrustedDisclosure sources={untrusted} />}
            </>
          )}
        </>
      )}
    </div>
  );
}

export default function Timeline({ steps }: { steps: AgentStep[] }) {
  if (!steps.length) return null;
  return (
    <div className="timeline" role="list" aria-label="Steps the agent took">
      {steps.map((step) => (
        <Step key={step.id} step={step} />
      ))}
    </div>
  );
}
