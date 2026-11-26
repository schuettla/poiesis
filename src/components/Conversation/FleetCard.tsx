import { useEffect, useState } from "react";
import type { SubRun } from "../../lib/types";
import { stillWorking } from "../../lib/api";
import { useAppStore } from "../../lib/store";
import { runLabel } from "../../lib/teamLive";
import { agentLogoUrl, evidenceLeft, evidenceLine, evidenceWarns } from "../../lib/engines";
import { useLedTeam } from "../../lib/useLedTeam";
import { usePersonaAppsLine } from "../Apps/personaAppsLine";
import AppLogo from "../Apps/AppLogo";
import BlockRenderer from "../Blocks/BlockRenderer";
import { ChevronIcon, SettingsIcon } from "../Icons/Icons";
import { ContinueThere, EngineChips, UndoHandover } from "./EngineChips";
import Orb from "../Orb/Orb";
import { orbForSubRun } from "../Orb/orbState";

/**
 * `SUB-UI-1`: the agents a turn handed work to, shown inside the turn itself.
 *
 * A progress bar would not be enough. The point of delegation is that several
 * agents are working *for you* at once, and the only way that is true rather
 * than merely claimed is if each one can be watched, told something, and
 * stopped — from here, while it runs.
 */

/** Keep the dot inside the app's own palette. Six colours would mean inventing
 * five; hashing onto the four accents that already exist keeps agents apart
 * without adding a second colour language to the product. */
const DOT_TOKENS = ["--local", "--cloud", "--ok", "--ink-faint"];

function dotColor(agent: string): string {
  let hash = 0;
  for (const ch of agent) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return `var(${DOT_TOKENS[hash % DOT_TOKENS.length]})`;
}

function clock(ms: number): string {
  const secs = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/** What a running child is doing right now: its newest step, else the last
 * line of prose it produced. A row that never changes reads as a hang. */
export function nowDoing(run: SubRun): string {
  // `SUB-12`: a queued child has taken no step and written no prose. Falling
  // through to "getting started" would claim it had begun.
  // `TEAM-14`: a team member waits in the team's queue the same way.
  // `PRF-5`: one that waits for a place another helper is changing says which.
  if (run.status === "queued" && run.queuedBecause) return run.queuedBecause;
  if (run.status === "queued" || run.waiting) return "waiting its turn";
  const step =
    [...run.steps].reverse().find((s) => s.status === "running") ??
    run.steps[run.steps.length - 1];
  if (step) return `${step.verb} ${step.target}`.trim();
  const lines = run.text.trim().split("\n").filter(Boolean);
  const line = lines[lines.length - 1];
  return line ? line.slice(0, 80) : "getting started";
}

/** How a finished child's row reads (PRES-0: first person, and honest about a
 * partial result rather than quietly showing a stump). */
export function endedLine(run: SubRun): string {
  const took = run.ms ? ` · ${clock(run.ms)}` : "";
  const steps = `${run.steps.length || ""}`.trim();
  const stepPart = steps ? ` · ${steps} steps` : "";
  switch (run.stopReason) {
    case "aborted":
      return `stopped, I kept what it had${stepPart}${took}`;
    case "timeout":
      return `ran out of time, this is what it had${stepPart}${took}`;
    case "max_steps":
      return `hit its step limit, this is what it had${stepPart}${took}`;
    case "budget":
      return `stopped at the team's limit, this is what it had${stepPart}${took}`;
    case "stuck":
      return `was not getting anywhere, this is what it had${stepPart}${took}`;
    case "error":
      return `it failed${stepPart}${took}`;
    case "plan_limit":
      return `hit its plan limit${stepPart}${took}`;
    default:
      return `done${stepPart}${took}`;
  }
}

function EvidenceLine({ run }: { run: SubRun }) {
  const e = run.evidence;
  if (!e) return null;
  const left = evidenceLeft(e);
  return (
    <p className={`fleet-evidence${evidenceWarns(e) ? " fleet-evidence-warn" : ""}`} title={left?.full}>
      {evidenceLine(e)}
      {left && ` ${left.text}`}
    </p>
  );
}

function FleetRow({ run, label }: { run: SubRun; label: string }) {
  const openItem = useAppStore((s) => s.openItem);
  const steerSubRun = useAppStore((s) => s.steerSubRun);
  const stopSubRun = useAppStore((s) => s.stopSubRun);
  const openEngines = useAppStore((s) => s.openEngines);
  // `CA-UI-6`: an agent backed by a persona narrowed to some apps says which.
  const personaId = useAppStore((s) => s.personas.find((p) => p.name === run.agent)?.id);
  const appsLine = usePersonaAppsLine(personaId);
  const [steering, setSteering] = useState(false);
  const [draft, setDraft] = useState("");
  const [sent, setSent] = useState(false);
  const [open, setOpen] = useState(false);
  const [, tick] = useState(0);

  const running = stillWorking(run.status);
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [running]);

  const send = async () => {
    const text = draft.trim();
    if (!text) return;
    setDraft("");
    setSteering(false);
    const delivered = await steerSubRun(run.runId, text);
    setSent(delivered);
  };

  return (
    <div className={`fleet-row ${run.status}`}>
      {/* Running (or queued): the orb says what this child is doing, so several
          at once read as several things happening. Ended: the agent's own dot. */}
      <span className="fleet-lead">
        {running ? (
          <Orb state={orbForSubRun(run)} />
        ) : (
          <span className="fleet-dot" style={{ background: dotColor(run.agent) }} aria-hidden="true" />
        )}
      </span>
      <span className="fleet-who">
        {run.engine && <AppLogo slug={run.engine.id} name={run.engine.name} src={agentLogoUrl(run.engine.logo)} />}
        <span className="fleet-agent" title={appsLine ? `${run.task}
${appsLine}` : run.task}>
          {label}
        </span>
        <EngineChips run={run} />
      </span>
      <span className="fleet-doing">{running ? nowDoing(run) : endedLine(run)}</span>
      {running && <span className="fleet-clock">{clock(Date.now() - run.startedAt)}</span>}
      <span className="fleet-actions">
        <button
          className="fleet-action"
          onClick={() => openItem({ kind: "run", id: run.runId })}
        >
          Open
        </button>
        {running && (
          <>
            <button className="fleet-action" onClick={() => setSteering((v) => !v)}>
              Steer
            </button>
            <button className="fleet-action" onClick={() => stopSubRun(run.runId)}>
              Stop
            </button>
          </>
        )}
        {!running && run.text && (
          <button
            className="fleet-action"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
          >
            Report
            <ChevronIcon dir={open ? "up" : "down"} size={10} strokeWidth={1.8} />

          </button>
        )}
        {run.engine && (
          <button
            className="fleet-action fleet-gear"
            title={`Rules for ${run.engine.name}`}
            aria-label={`Rules for ${run.engine.name}`}
            onClick={() => run.engine && openEngines(run.engine.id)}
          >
            <SettingsIcon size={13} />
          </button>
        )}
      </span>
      {steering && (
        <form
          className="fleet-steer"
          onSubmit={(e) => {
            e.preventDefault();
            send();
          }}
        >
          <input
            autoFocus
            value={draft}
            placeholder={
              run.engine && !run.engine.can_steer
                ? "I'll pass this on when it's done with this job"
                : "Tell this agent something"
            }
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setSteering(false);
            }}
          />
        </form>
      )}
      {sent && running && (
        <p className="fleet-note">
          {run.engine && !run.engine.can_steer
            ? "Sent. I'll pass this on when it's done with this job."
            : "Sent. It will pick this up after the step it is on."}
        </p>
      )}
      {/* `MCPS-2`: what the agent put on its card with `present`. */}
      {run.blocks && run.blocks.length > 0 && (
        <div className="fleet-blocks">
          {run.blocks.map((b) => (
            <BlockRenderer key={b.id} block={b} />
          ))}
        </div>
      )}
      {/* `PRF-2`, `PRF-4`: what the job changed, whether the check ran after it, what it left open. */}
      {run.evidence && !running && <EvidenceLine run={run} />}
      {open && !running && <pre className="fleet-report">{run.text}</pre>}
      {!running && run.engine && (
        <div className="engine-after">
          <ContinueThere run={run} />
          <UndoHandover run={run} />
        </div>
      )}
    </div>
  );
}

export default function FleetCard({ runIds }: { runIds: string[] }) {
  const subRuns = useAppStore((s) => s.subRuns);
  const runs = runIds.map((id) => subRuns[id]).filter(Boolean);
  // `TEAM-14`: in a team's session a member is "api · Atlas", its role and its persona.
  const { team } = useLedTeam(runs.some((r) => r.member) ? runs[0].parentConversationId : null);
  const personas = useAppStore((s) => s.personas);
  if (runs.length === 0) return null;
  return (
    <div className="fleet-card">
      <div className="fleet-header">Agents I started</div>
      {runs.map((run) => (
        <FleetRow key={run.runId} run={run} label={runLabel(run, team, personas)} />
      ))}
    </div>
  );
}
