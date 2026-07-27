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

/** Where a recalled answer actually came from (RCL-UI). Chat rows jump to the
 *  source conversation; memory rows name the entry that was matched. */
function Provenance({ matches }: { matches: SearchHit[] }) {
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);

  return (
    <div className="recall-matches">
      {matches.map((m, i) => {
        const body = (
          <>
            <span className={`recall-chip ${m.source}`}>
              {m.source === "memory" ? "◆ memory" : "chat"}
            </span>
            <span className="recall-title">{m.title}</span>
            <span className="recall-date">{asDate(m.created_at)}</span>
            <span className="recall-snippet">{m.snippet}</span>
          </>
        );
        const convId = m.conversation_id;
        return convId ? (
          <button
            key={`${m.source}-${i}`}
            className="recall-row link"
            onClick={() => setActiveConversation(convId)}
            title="Open this conversation"
          >
            {body}
          </button>
        ) : (
          <div key={`${m.source}-${i}`} className="recall-row">
            {body}
          </div>
        );
      })}
    </div>
  );
}

function Step({ step }: { step: AgentStep }) {
  const [open, setOpen] = useState(false);
  const matches = step.matches ?? [];

  return (
    <div className={`step ${step.status}`} role="listitem" aria-label={announce(step)}>
      <span className="verb" aria-hidden="true">
        {step.verb}
      </span>
      <span className="target" aria-hidden="true">
        {step.target}
      </span>
      {step.result && (
        <span className="result" aria-hidden="true">
          {step.result}
        </span>
      )}
      {matches.length > 0 && (
        <>
          <button
            className="step-expand"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            aria-label={`${open ? "Hide" : "Show"} the ${matches.length} sources for this recall`}
          >
            {open ? "⌃" : "⌄"}
          </button>
          {open && <Provenance matches={matches} />}
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
