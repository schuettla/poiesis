import { useEffect, useState } from "react";
import type { SubRun } from "../../lib/types";
import * as api from "../../lib/api";
import { becauseChip, matchedTitle, payChip } from "../../lib/engines";
import { useAppStore } from "../../lib/store";

/**
 * What a Fleet row and a job's own view say about a child an agent app ran
 * (`CLI-UI-2`, `RTE-UI-3`, `CLI-UI-5`): who paid, which of the user's rules sent
 * the work there, and the two ways out of it: take it back, or pick it up in a
 * terminal. Plain words; the agent's name only as text (`TRM-3`).
 */

/** `on your plan` and `because U2`. The second opens that agent's rules. */
export function EngineChips({ run }: { run: SubRun }) {
  const openEngines = useAppStore((s) => s.openEngines);
  const engine = run.engine;
  if (!engine) return null;
  const pay = payChip(engine.paid_by);
  const why = becauseChip(engine.because);
  const words = engine.because_text ?? WHY_TITLE[engine.because ?? ""] ?? "";
  // `USE-13`: a small dot when a quick decision matched the rule before I started.
  const early = engine.matched != null;
  return (
    <>
      {pay && <span className="fleet-chip">{pay}</span>}
      {why && (
        <button
          className="fleet-chip fleet-chip-why"
          title={early ? [matchedTitle(engine.matched!), words].filter(Boolean).join("\n") : words}
          onClick={() => openEngines(engine.id)}
        >
          {why}
          {early && <span className="fleet-chip-dot" aria-hidden="true" />}
        </button>
      )}
    </>
  );
}

const WHY_TITLE: Record<string, string> = {
  asked: "You named this agent.",
  persona: "A persona you set up runs on this agent.",
};

/** `RTE-UI-3`: "This shouldn't have gone to Claude Code". Stops it if it still
 * runs, counts a no against the rule it cited, and tells me to do it myself. */
export function UndoHandover({ run }: { run: SubRun }) {
  const [done, setDone] = useState(false);
  const engine = run.engine;
  // Only a rule can be wrong about this. You naming the agent, or a persona you set up, is not a rule.
  if (!engine || !engine.because || engine.because === "asked" || engine.because === "persona") return null;
  if (done) return <span className="fleet-note">Noted. I'll count that against {engine.because}.</span>;
  return (
    <button
      className="fleet-action"
      onClick={() => {
        setDone(true);
        void api.undoEngineHandover(run.runId).catch(() => setDone(false));
      }}
    >
      This shouldn&apos;t have gone to {engine.name}
    </button>
  );
}

/** `CLI-UI-5`: the work is not trapped here. Copies the command that resumes the
 * agent's own session and opens a terminal in the folder. The command is copied
 * even when the terminal cannot be opened. */
export function ContinueThere({ run }: { run: SubRun }) {
  const [session, setSession] = useState<string | undefined>(run.engineSession);
  const [note, setNote] = useState<string | null>(null);
  const ended = !api.stillWorking(run.status);
  const isEngine = !!run.engine;

  useEffect(() => {
    if (!isEngine || session || !ended || !api.inTauri()) return;
    let live = true;
    api
      .getSubagentRun(run.runId)
      .then((r) => live && r?.engine_session && setSession(r.engine_session))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [isEngine, session, ended, run.runId]);

  if (!run.engine || !session) return null;
  const name = run.engine.name;

  const go = async () => {
    try {
      const r = await api.continueEngineJob(run.runId);
      try {
        await navigator.clipboard.writeText(r.command);
        setNote(r.opened ? "Opened a terminal. The command is copied too." : `Copied: ${r.command}`);
      } catch {
        setNote(r.command);
      }
    } catch (e) {
      setNote(String(e).replace(/^Error:\s*/, ""));
    }
  };

  return (
    <div className="engine-continue">
      <span>I handed this to {name}.</span>
      <button className="fleet-action" onClick={() => void go()}>
        Continue there
      </button>
      {note && <span className="fleet-note" role="status">{note}</span>}
    </div>
  );
}
