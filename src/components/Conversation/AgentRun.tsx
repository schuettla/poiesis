import type { Message } from "../../lib/types";
import { useAppStore } from "../../lib/store";
import Timeline from "./Timeline";
import RunText from "./RunText";
import ChatImage from "./ChatImage";
import BlockRenderer from "../Blocks/BlockRenderer";
import ProposalCard from "./ProposalCard";

/** Three-dot pulse shown while the agent has produced nothing visible yet
 * (no token, no step, no block). Without this the turn looks stalled — the
 * composer's stop button was the only sign anything was happening. */
function Thinking() {
  return (
    <div className="thinking" role="status" aria-label="Agent is working">
      <span className="thinking-dot" />
      <span className="thinking-dot" />
      <span className="thinking-dot" />
    </div>
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

export default function AgentRun({ message }: { message: Message }) {
  const model = message.model;
  const images = message.attachments?.filter((a) => a.kind === "image") ?? [];
  const hasContent =
    !!message.text || images.length > 0 || !!message.steps?.length || !!message.blocks?.length;
  const isThinking = message.streaming && !hasContent;

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
      {images.map((a) => (
        <ChatImage key={a.id} path={a.path} dataUri={a.dataUri} />
      ))}
      {isThinking && <Thinking />}
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
