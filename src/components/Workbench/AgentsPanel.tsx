import { useEffect, useState } from "react";
import type { SubRun } from "../../lib/types";
import * as api from "../../lib/api";
import { useAppStore } from "../../lib/store";
import Timeline from "../Conversation/Timeline";
import RunText from "../Conversation/RunText";
import { ContinueThere, EngineChips, UndoHandover } from "../Conversation/EngineChips";
import Orb from "../Orb/Orb";
import { orbForSubRun } from "../Orb/orbState";
import OrgDiagram from "./OrgDiagram";

/**
 * `SUB-UI-2`, split by `SHL-22`: the Agents sub-view is the run list, an
 * overview that lives in the sidebar. One child's live transcript is a single
 * item, so pressing a row opens it as a tab in the strip (`RunView`), at the
 * width of the conversation rather than squeezed under the list.
 *
 * `WRK-15`: it is also the Coordination view. The same agents, now inside their
 * organization: above the list, a diagram with a shape for the way of working, drawn
 * from the run's own events (`OrgDiagram`).
 *
 * This is what makes a child feel like an agent rather than a spinner. A row
 * tells you one is working; only opening it tells you what it is actually
 * doing, and lets you decide whether to redirect it.
 */

function ended(run: SubRun): boolean {
  return !api.stillWorking(run.status);
}

/** The child's own artifacts, read from its own conversation. They belong to
 * it, not to the turn that started it — folding them into the lead's message
 * would claim the lead made them. */
function ChildArtifacts({ conversationId }: { conversationId: string }) {
  const [artifacts, setArtifacts] = useState<api.Artifact[]>([]);
  useEffect(() => {
    let live = true;
    api
      .listArtifacts(conversationId)
      .then((rows) => live && setArtifacts(rows))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [conversationId]);
  if (artifacts.length === 0) return null;
  return (
    <div className="agents-artifacts">
      {artifacts.map((a) => (
        <span key={a.id} className="agents-artifact">
          {a.title}
        </span>
      ))}
    </div>
  );
}

export default function AgentsPanel({ conversationId }: { conversationId: string }) {
  const subRuns = useAppStore((s) => s.subRuns);
  const activeItemId = useAppStore((s) => s.activeItemId);
  const openItem = useAppStore((s) => s.openItem);
  const loadSubRuns = useAppStore((s) => s.loadSubRuns);

  useEffect(() => {
    loadSubRuns(conversationId).catch(() => {});
  }, [conversationId, loadSubRuns]);

  const runs = Object.values(subRuns)
    .filter((r) => r.parentConversationId === conversationId)
    .sort((a, b) => a.startedAt - b.startedAt);

  return (
    <div className="agents-panel">
      <OrgDiagram conversationId={conversationId} />
      {runs.length === 0 && (
        <div className="wb-empty">
          <p className="wb-empty-title">No agents yet</p>
          <p className="wb-empty-blurb">I have not handed any work out in this conversation yet.</p>
        </div>
      )}
      {runs.length > 0 && (
        <div className="agents-list-head">
          <h3 className="agents-list-title">Agents</h3>
          <span className="agents-list-count">{runs.length}</span>
          <p className="agents-list-blurb">Helpers I started for this chat. Open one to follow its work.</p>
        </div>
      )}
      <div className="agents-list" aria-label="Agents I started">
        {runs.map((run) => {
          const open = activeItemId === `run:${run.runId}`;
          return (
            <button
              key={run.runId}
              aria-current={open || undefined}
              className={`agents-row ${open ? "active" : ""} ${run.status}`}
              onClick={() => openItem({ kind: "run", id: run.runId })}
            >
              {ended(run) ? (
                <span className="agents-row-dot" aria-label="finished" />
              ) : (
                <span className="agents-row-orb" role="img" aria-label="working">
                  <Orb state={orbForSubRun(run)} />
                </span>
              )}
              <span className="agents-row-agent">{run.agent}</span>
              <span className="agents-row-task">{run.task}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** One child agent, open as an item tab (`SHL-22`). */
export function RunView({ run }: { run: SubRun }) {
  const stopSubRun = useAppStore((s) => s.stopSubRun);
  const steerSubRun = useAppStore((s) => s.steerSubRun);
  const [draft, setDraft] = useState("");

  return (
    <div className="agents-detail">
      <div className="agents-detail-col">
        <div className="agents-detail-head">
          <span className="agents-detail-agent">{run.agent}</span>
          <span className={`agents-detail-state ${ended(run) ? "done" : "live"}`}>
            {ended(run) ? "Finished" : "Working"}
          </span>
          <EngineChips run={run} />
          {!ended(run) && (
            <button className="agents-detail-stop" onClick={() => stopSubRun(run.runId)}>
              Stop
            </button>
          )}
        </div>
        {ended(run) && run.engine && (
          <div className="engine-after">
            <ContinueThere run={run} />
            <UndoHandover run={run} />
          </div>
        )}
        <section className="agents-task" aria-label="Task">
          <p className="agents-section-label">Task</p>
          <p className="agents-task-text">{run.task}</p>
        </section>
        {run.steps.length > 0 && <Timeline steps={run.steps} />}
        {run.text && <RunText text={run.text} streaming={!ended(run)} />}
        <ChildArtifacts conversationId={run.conversationId} />
        {!ended(run) && (
          <form
            className="agents-steer"
            onSubmit={(e) => {
              e.preventDefault();
              const text = draft.trim();
              if (!text) return;
              setDraft("");
              steerSubRun(run.runId, text);
            }}
          >
            <input
              value={draft}
              placeholder={
                run.engine && !run.engine.can_steer
                  ? "I'll pass this on when it's done with this job"
                  : "Tell this agent something"
              }
              onChange={(e) => setDraft(e.target.value)}
            />
          </form>
        )}
        {run.steerPending && <p className="fleet-note">Sent. It will pick this up after the step it is on.</p>}
      </div>
    </div>
  );
}
