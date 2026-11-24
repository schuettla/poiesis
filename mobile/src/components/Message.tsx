import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import * as link from "../link";
import type { PhoneArtifact, PhoneMessage } from "../types";

/** A picture that lives on the desktop. It is fetched through the link when it comes into view. */
export function DesktopImage({ path, alt }: { path: string; alt: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    link
      .call<{ mime: string; data: string }>("read_media", { path })
      .then((m) => live && setSrc(`data:${m.mime};base64,${m.data}`))
      .catch((e) => live && setFailed(link.words(e)));
    return () => {
      live = false;
    };
  }, [path]);
  if (failed) return <p className="chat-note">{failed}</p>;
  return src ? <img className="chat-image" src={src} alt={alt} /> : <div className="chat-image chat-image-wait" aria-hidden />;
}

/** Markdown for an answer. Links are plain text: the phone does not open the desktop's pages. */
export function Answer({ text }: { text: string }) {
  return (
    <div className="answer">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: ({ children }) => <span>{children}</span> }}>
        {text}
      </ReactMarkdown>
    </div>
  );
}

/** A made thing: a titled card. Text opens read only; a picture shows in place. */
export function ArtifactCard({
  artifact,
  onOpen,
}: {
  artifact: Pick<PhoneArtifact, "id" | "title" | "kind" | "content">;
  onOpen: () => void;
}) {
  if (artifact.kind === "image") return <DesktopImage path={artifact.content} alt={artifact.title} />;
  if (artifact.kind === "video") return <p className="chat-note">{artifact.title}: open this on your desktop to see it.</p>;
  return (
    <button className="artifact-card" onClick={onOpen}>
      <span className="artifact-title">{artifact.title}</span>
      <span className="artifact-kind">Tap to read</span>
    </button>
  );
}

export default function MessageView({ message }: { message: PhoneMessage }) {
  const mine = message.role === "user";
  const pictures = (message.attachments ?? []).filter((a) => a.kind === "image");
  return (
    <article className={`message ${mine ? "message-user" : "message-assistant"}`}>
      {pictures.map((p) => (
        <DesktopImage key={p.path} path={p.path} alt={p.name} />
      ))}
      {mine ? <p className="message-text">{message.content}</p> : message.content ? <Answer text={message.content} /> : null}
    </article>
  );
}
