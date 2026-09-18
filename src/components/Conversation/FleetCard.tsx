import { useEffect, useState } from "react";
import type { SubRun } from "../../lib/types";
import { stillWorking } from "../../lib/api";
import { useAppStore } from "../../lib/store";

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
function nowDoing(run: SubRun): string {
  // `SUB-12`: a queued child has taken no step and written no prose. Falling
  // through to "getting started" would claim it had begun.
  if (run.status === "queued") return "waiting its turn";
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
function endedLine(run: SubRun): string {
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
    case "error":
      return `it failed${stepPart}${took}`;
    default:
      return `done${stepPart}${took}`;
  }
}

function FleetRow({ run }: { run: SubRun }) {
  const openItem = useAppStore((s) => s.openItem);
  const steerSubRun = useAppStore((s) => s.steerSubRun);
  const stopSubRun = useAppStore((s) => s.stopSubRun);
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
      <span className="fleet-dot" style={{ background: dotColor(run.agent) }} aria-hidden="true" />
      <span className="fleet-agent" title={run.task}>
        {run.agent}
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
            Report {open ? "⌃" : "⌄"}
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
            placeholder="Tell this agent something"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setSteering(false);
            }}
          />
        </form>
      )}
      {sent && running && (
        <p className="fleet-note">Sent. It will pick this up after the step it is on.</p>
      )}
      {open && !running && <pre className="fleet-report">{run.text}</pre>}
    </div>
  );
}

export default function FleetCard({ runIds }: { runIds: string[] }) {
  const subRuns = useAppStore((s) => s.subRuns);
  const runs = runIds.map((id) => subRuns[id]).filter(Boolean);
  if (runs.length === 0) return null;
  return (
    <div className="fleet-card">
      <div className="fleet-header">Agents I started</div>
      {runs.map((run) => (
        <FleetRow key={run.runId} run={run} />
      ))}
    </div>
  );
}
