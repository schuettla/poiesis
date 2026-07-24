import type { Message } from "../../lib/types";
import Timeline from "./Timeline";
import RunText from "./RunText";
import ChatImage from "./ChatImage";
import BlockRenderer from "../Blocks/BlockRenderer";

export default function AgentRun({ message }: { message: Message }) {
  const model = message.model;
  const images = message.attachments?.filter((a) => a.kind === "image") ?? [];
  return (
    <div className="agent-run">
      {model && (
        <div className="run-header">
          <span className={`provenance-dot ${model.provenance}`} aria-hidden="true" />
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
      {!message.text && images.length === 0 && message.streaming && (
        <RunText text="" streaming />
      )}
    </div>
  );
}
