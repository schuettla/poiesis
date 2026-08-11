import type { Message } from "../../lib/types";
import { isPersistedId, useAppStore } from "../../lib/store";
import Timeline from "./Timeline";
import RunText from "./RunText";
import ChatMedia, { ChatMediaPending } from "./ChatMedia";
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

export default function AgentRun({ message }: { message: Message }) {
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
    !!message.blocks?.length;

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
  const saidNothing = !message.streaming && !hasContent;

  return (
    <div className="agent-run">
      {model && (
        <div className="run-header">
          <span className={`provenance-dot ${model.provenance}`} aria-hidden="true" />
          <span className="agent-label">Agent</span>
          <span className="model-tag">{model.name}</span>
        </div>
      )}
      {message.steps && <Timeline steps={message.steps} />}
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
      {saidNothing && <SaidNothing />}
      {!message.streaming && message.text && <WhyThisAnswer messageId={message.id} />}
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
