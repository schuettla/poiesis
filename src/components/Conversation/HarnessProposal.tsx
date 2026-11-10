import type { HarnessProposalView } from "../../lib/types";
import type { PresetView } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "./HarnessProposal.css";

/** What I would like to do, in the first person (`CPX-1`). */
function wording(p: HarnessProposalView, presets: PresetView[]): { ask: string; yes: string; no: string } {
  if (p.name === "switch_mode" && p.payload.style) {
    // `WRK-13`: another way of working, as II.9 words it: the reason, then the question.
    const preset = presets.find((x) => x.slug === p.payload.mode);
    const name = preset?.name ?? String(p.payload.mode);
    const line = preset?.line ? `: ${preset.line.charAt(0).toLowerCase()}${preset.line.slice(1)}` : "";
    return { ask: `${p.reason} Shall I switch to ${name}${line}?`, yes: "Switch", no: "Keep going like this" };
  }
  if (p.name === "switch_mode") {
    const mode = p.payload.mode === "plan_first" ? "Plan first" : "Workspace";
    return { ask: `I'd like to switch to ${mode}: ${p.reason}`, yes: "Switch", no: "Not now" };
  }
  if (p.name === "schedule") {
    return {
      ask: `I'd like to run this ${p.payload.when ?? "later"}: ${p.payload.task ?? ""}`,
      yes: "Set it up",
      no: "Not now",
    };
  }
  return { ask: `I'd like to make room: ${p.reason}`, yes: "Go ahead", no: "Not now" };
}

/**
 * `AGC-4`: something I want to change that is yours, a mode or a schedule.
 *
 * The run did not wait for this. It carried on, and you answer when you like. A
 * yes carries the change out in your session (and, for a schedule, only opens a
 * draft: nothing is scheduled until you press Save there). A not now leaves
 * everything as it was and is remembered, so I learn what you do not want asked.
 */
export default function HarnessProposal({ proposal }: { proposal: HarnessProposalView }) {
  const resolve = useAppStore((s) => s.resolveHarnessProposal);
  const presets = useAppStore((s) => s.presets);
  const { ask, yes, no } = wording(proposal, presets);
  return (
    <div className="harness-proposal" role="group" aria-label="A change I would like to make">
      <p className="harness-proposal-text">
        <span aria-hidden="true">◆ </span>
        {ask}
      </p>
      <div className="harness-proposal-actions">
        <button className="btn-text" onClick={() => void resolve(proposal.id, true)}>
          {yes}
        </button>
        <button className="btn-text" onClick={() => void resolve(proposal.id, false)}>
          {no}
        </button>
      </div>
    </div>
  );
}
