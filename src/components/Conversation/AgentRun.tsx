import { useEffect, useState } from "react";
import type { Message } from "../../lib/types";
import type { StopReason } from "../../lib/api";
import { isPersistedId, useAppStore } from "../../lib/store";
import Timeline from "./Timeline";
import RunText from "./RunText";
import ChatMedia, { ChatMediaPending } from "./ChatMedia";
import FleetCard from "./FleetCard";
import PlanCard from "./PlanCard";
import BlockRenderer from "../Blocks/BlockRenderer";
import ProposalCard from "./ProposalCard";
import "../Context/Context.css";

/** Three-dot pulse shown while the turn is live but nothing else on screen is
 * moving. Without this the turn looks stalled — the composer's stop button was
 * the only sign anything was happening.
 *
 * `label` names what it's waiting on once some steps have already run: after a
 * browse-click-read sequence, three bare dots don't distinguish "thinking
 * about the next move" from "hung". */
function Thinking({ label }: { label?: string }) {
  return (
    <div className="thinking" role="status" aria-label={label ?? "Agent is working"}>
      <span className="thinking-dot" />
      <span className="thinking-dot" />
      <span className="thinking-dot" />
      {label && <span className="thinking-label">{label}</span>}
    </div>
  );
}

/** A turn that ended having said nothing at all.
 *
 * Small models do this routinely after a run of tool calls — especially one
 * that ended in failures. An empty bubble is indistinguishable from a turn
 * that's still going, which is the worst thing it could look like, so the end
 * of the turn is stated outright. */
function SaidNothing() {
  return (
    <p className="run-empty">
      I stopped without saying anything. Ask me again, or tell me what to do with what I found.
    </p>
  );
}

/** A turn that ran its steps and then wrote no answer. A different failure
 * from saying nothing at all: the work is on screen, only the report is
 * missing. It happens when a model returns an empty reply after its tool
 * results, even after the loop has asked it once more for the answer. */
function NoAnswer() {
  return (
    <p className="run-no-answer">
      I did the steps above but ended without writing an answer. Ask me what they showed.
    </p>
  );
}

/** `HRN-UI-3`: a quiet line under the timeline while the run is live, so the
 * time between steps reads as work rather than as a hang.
 *
 * It used to read "step 1 of 12". That was the step *budget*, and as a live
 * status line it was worse than useless: it told you nothing about what the run
 * was doing, and the 12 invited the reading that the app had planned twelve
 * steps — which it had not. The budget still exists and is still settable
 * (Settings → Tools), but it belongs where a limit belongs: in the setting, and
 * in the message you get if a run ever hits it. What goes here is what the run
 * is doing now.
 *
 * Only rendered for the turn that is actually streaming — a finished turn's
 * meter would be a number about nothing. */
function RunMeter({ steps }: { steps?: Message["steps"] }) {
  const run = useAppStore((s) => s.activeRun);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!run) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [run]);
  if (!run || run.step < 1) return null;
  const secs = Math.floor((Date.now() - run.startedAt) / 1000);
  const clock = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
  // `OBS-3`: a percentage only where the window is actually known. Where it is
  // not, the raw estimate still says something useful and claims nothing.
  const context =
    run.contextTokens > 0
      ? run.contextWindow
        ? `context ${Math.min(100, Math.round((run.contextTokens / run.contextWindow) * 100))}%`
        : `about ${Math.round(run.contextTokens / 1000)}k in context`
      : null;
  const tight = !!run.contextWindow && run.contextTokens / run.contextWindow > 0.8;
  // What is happening right now, in this order of confidence: a tool that is
  // actually running, then thinking we can see arriving, then the honest
  // fallback — which names what the loop is really doing between steps, since
  // "working" beside a clock reading 10:56 tells you nothing about whether
  // anything is still alive.
  const running = steps?.find((s) => s.status === "running");
  const activity = running
    ? `${running.verb} ${running.target}`.trim()
    : run.thinking
      ? "thinking"
      : "waiting for the model";
  // `PLN-UI-2`: the plan item says what the work is *for*, which the tool line
  // never does — but it is the frame, not the pulse. It goes in front of the
  // activity rather than replacing it: on its own it stood still for as long as
  // an item took, and a stalled run was indistinguishable from a working one.
  // Trimmed, because the card above already carries the item in full.
  const item = run.plan?.items.find((i) => i.status === "doing");
  const doing = item ? `${trim(item.text)} · ${activity}` : activity;
  return (
    <>
      <p className={`run-meter ${tight ? "tight" : ""}`}>
        {doing} · {clock}
        {context ? ` · ${context}` : ""}
      </p>
      <ThinkingTrace text={run.thinking} />
    </>
  );
}

/** Enough of a plan item to recognise it. The whole thing belongs on the card
 * above, not in a one-line meter that also has to carry a clock. */
function trim(text: string, max = 44): string {
  return text.length > max ? `${text.slice(0, max).trimEnd()}…` : text;
}

/** The model's thinking while it is thinking.
 *
 * A reasoning model can spend minutes here before its first word, and the meter
 * above it was the only sign of life — a clock ticking beside an empty screen,
 * which reads as a hang. This is folded shut by default because thinking is not
 * the answer and should not compete with it, but it is openable, because the
 * only thing worse than a wall of reasoning is no evidence at all. */
function ThinkingTrace({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  if (!text) return null;
  return (
    <details className="run-thinking" open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>Thinking ({text.length.toLocaleString()} characters so far)</summary>
      {/* The tail, not the head: what it is thinking about now is the part
          that says whether it is still going anywhere. */}
      <pre>{text.slice(-4000)}</pre>
    </details>
  );
}

/** `HRN-3`: why a turn stopped, when it stopped for anything but finishing.
 * The text above is what the run had in hand, and saying so is the difference
 * between a short answer and a truthful one. */
function StoppedNote({ reason }: { reason: StopReason }) {
  const said: Record<StopReason, string> = {
    completed: "",
    aborted: "You stopped me. This is what I had.",
    timeout: "I ran out of time. This is what I had.",
    max_steps: "I stopped at my step limit. This is what I had.",
    error: "The model call failed partway. This is what I had.",
  };
  const text = said[reason];
  if (!text) return null;
  return <p className="run-stopped">{text}</p>;
}

/** Inline chips for artifacts produced during this turn (CHT-6). Clicking one
 * opens the Workbench straight on that artifact instead of always landing on
 * whichever one is currently selected. */
function ArtifactChips({ ids }: { ids: string[] }) {
  const convId = useAppStore((s) => s.activeConversationId);
  const artifacts = useAppStore((s) => (convId ? s.artifacts[convId] ?? [] : []));
  const openArtifact = useAppStore((s) => s.openArtifact);

  return (
    <div className="artifact-chips">
      {ids.map((id) => {
        const artifact = artifacts.find((a) => a.id === id);
        if (!artifact) return null;
        return (
          <button
            key={id}
            className="artifact-chip"
            onClick={() => openArtifact(id)}
            title={`Open “${artifact.title}” in the Workbench`}
          >
            <span className="artifact-chip-kind">{artifact.kind}</span>
            <span className="artifact-chip-title">{artifact.title}</span>
            <span className="artifact-chip-arrow" aria-hidden="true">→</span>
          </button>
        );
      })}
    </div>
  );
}

/** Files the agent changed during this turn. This is the link that makes the
 * Workbench feel connected to what just happened rather than a separate
 * browser: click to see the file, Undo to put it back. */
function ChangedFiles({ ids }: { ids: string[] }) {
  const trash = useAppStore((s) => s.trash);
  const selectNode = useAppStore((s) => s.selectNode);
  const setDockOpen = useAppStore((s) => s.setDockOpen);
  const undoFileOp = useAppStore((s) => s.undoFileOp);

  const entries = ids.map((id) => trash.find((t) => t.id === id)).filter(Boolean);
  if (entries.length === 0) return null;

  return (
    <div className="changed-files">
      {entries.map((t) => (
        <div key={t!.id} className={`changed-file ${t!.undone ? "undone" : ""}`}>
          <button
            className="changed-file-name"
            title={t!.path}
            onClick={() => {
              setDockOpen(true);
              selectNode({ kind: "file", id: t!.path });
            }}
          >
            {t!.path.split(/[\\/]/).pop()}
          </button>
          {t!.undone ? (
            <span className="changed-file-undone">undone</span>
          ) : (
            <button className="changed-file-undo" onClick={() => undoFileOp(t!.id)}>
              Undo
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

/** WHY-4's second entry point: opens the shared `ContextPanel` for exactly
 * this message's stored manifest, once it has one to show (a still-streaming
 * or purely optimistic message hasn't finalized yet). */
function WhyThisAnswer({ messageId }: { messageId: string }) {
  const convId = useAppStore((s) => s.activeConversationId);
  const openContextPanel = useAppStore((s) => s.openContextPanel);
  if (!convId || !isPersistedId(messageId)) return null;
  return (
    <button
      className="why-link"
      onClick={() => openContextPanel({ conversationId: convId, messageId })}
    >
      why this answer?
    </button>
  );
}

/**
 * `HRN-UI-5`: the two things you can do with an answer once it exists.
 *
 * **Try again from here** branches the chat just before this turn and asks the
 * question again, so a second attempt costs nothing that already worked — the
 * original stays exactly where it was.
 *
 * **Continue where I stopped** appears only on a run that ended at a limit or
 * was interrupted, and only on the last turn, because "continue" has one
 * meaning and it is the run at the end. It picks that run back up with its tool
 * results intact instead of paying for them twice.
 */
function TurnActions({ message, last }: { message: Message; last: boolean }) {
  const forkFromMessage = useAppStore((s) => s.forkFromMessage);
  const resumeLastRun = useAppStore((s) => s.resumeLastRun);
  const busy = useAppStore((s) => s.busy);
  if (message.streaming || busy || !isPersistedId(message.id)) return null;

  const interrupted =
    message.stopReason === "aborted" ||
    message.stopReason === "timeout" ||
    message.stopReason === "max_steps";

  return (
    <div className="turn-actions">
      <button className="why-link" onClick={() => void forkFromMessage(message.id)}>
        Try again from here
      </button>
      {last && interrupted && (
        <button className="why-link" onClick={() => void resumeLastRun()}>
          Continue where I stopped
        </button>
      )}
    </div>
  );
}

export default function AgentRun({ message, last = false }: { message: Message; last?: boolean }) {
  const model = message.model;
  // Video counts as media here too — filtering to `"image"` used to drop a
  // generated clip on the floor, leaving the turn showing only its timeline
  // step while the MP4 sat in Library.
  const media = message.attachments?.filter((a) => a.kind === "image" || a.kind === "video") ?? [];
  const hasContent =
    !!message.text ||
    media.length > 0 ||
    !!message.pendingMedia ||
    !!message.steps?.length ||
    !!message.blocks?.length ||
    // A turn that wrote a plan and nothing else has still shown you something.
    !!message.plan?.items.length;

  // Something on screen is already moving when a step is in flight (its dot
  // pulses) or when prose is arriving (it carries a blinking caret). The gap
  // this closes is the third case: steps have finished and the model is
  // deciding what to do next, with nothing changing anywhere. That is most of
  // the wall-clock time in a browsing run, and it used to look identical to a
  // hang — the old test keyed on *any* step existing, so the indicator
  // switched off permanently the moment the first one landed.
  const anyStepRunning = message.steps?.some((s) => s.status === "running") ?? false;
  const isThinking = !!message.streaming && !anyStepRunning && !message.text;
  const thinkingLabel = message.steps?.length ? "still working" : undefined;
  // A run that stopped at a limit already says why it has nothing; the generic
  // "I said nothing" line under it would be the same news told worse.
  const saidNothing = !message.streaming && !hasContent && !message.stopReason;
  const noAnswer =
    !message.streaming &&
    !message.text &&
    !!message.steps?.length &&
    !media.length &&
    !message.pendingMedia &&
    !message.blocks?.length &&
    !message.artifactIds?.length &&
    (!message.stopReason || message.stopReason === "completed");

  return (
    <div className="agent-run">
      {model && (
        <div className="run-header">
          <span className={`provenance-dot ${model.provenance}`} aria-hidden="true" />
          <span className="agent-label">Agent</span>
          <span className="model-tag">{model.name}</span>
        </div>
      )}
      {/* `PLN-UI-1`: above the timeline, because the plan is what the steps
          below it are for. */}
      {message.plan && <PlanCard plan={message.plan} />}
      {message.steps && <Timeline steps={message.steps} live={!!message.streaming} />}
      {message.subRunIds && message.subRunIds.length > 0 && (
        <FleetCard runIds={message.subRunIds} />
      )}
      {message.blocks?.map((b) => (
        <BlockRenderer key={b.id} block={b} />
      ))}
      {message.text && <RunText text={message.text} streaming={message.streaming} />}
      {media.map((a) => (
        <ChatMedia
          key={a.id}
          attachment={a}
          alt={message.steps?.length ? message.steps[message.steps.length - 1].target : undefined}
        />
      ))}
      {message.pendingMedia && (
        <ChatMediaPending
          modality={message.pendingMedia.modality}
          aspectRatio={message.pendingMedia.aspectRatio}
          startedAt={message.pendingMedia.startedAt}
          jobId={message.pendingMedia.jobId}
        />
      )}
      {isThinking && <Thinking label={thinkingLabel} />}
      {message.streaming && <RunMeter steps={message.steps} />}
      {!message.streaming && message.stopReason && <StoppedNote reason={message.stopReason} />}
      {saidNothing && <SaidNothing />}
      {noAnswer && <NoAnswer />}
      {!message.streaming && message.text && <WhyThisAnswer messageId={message.id} />}
      <TurnActions message={message} last={last} />
      {message.artifactIds && message.artifactIds.length > 0 && (
        <ArtifactChips ids={message.artifactIds} />
      )}
      {message.fileChangeIds && message.fileChangeIds.length > 0 && (
        <ChangedFiles ids={message.fileChangeIds} />
      )}
      {message.proposalIds?.map((id) => (
        <ProposalCard key={id} id={id} />
      ))}
    </div>
  );
}
