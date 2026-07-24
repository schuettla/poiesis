import type { AgentStep } from "../../lib/types";

/** Screen-reader sentence for a step (§5.5: "searched project files, 3 matches"). */
function announce(step: AgentStep): string {
  const result = step.result ? `, ${step.result.replace(/^—\s*/, "")}` : "";
  const state = step.status === "running" ? " (in progress)" : "";
  return `${step.verb} ${step.target}${result}${state}`;
}

export default function Timeline({ steps }: { steps: AgentStep[] }) {
  if (!steps.length) return null;
  return (
    <div className="timeline" role="list" aria-label="Steps the agent took">
      {steps.map((step) => (
        <div key={step.id} className={`step ${step.status}`} role="listitem" aria-label={announce(step)}>
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
        </div>
      ))}
    </div>
  );
}
