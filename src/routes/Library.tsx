import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useAppStore } from "../lib/store";
import { readImageDataUri } from "../lib/api";
import type { Artifact } from "../lib/api";
import "./Surface.css";
import "./Library.css";

export default function Library() {
  const allArtifacts = useAppStore((s) => s.allArtifacts);
  const refreshAllArtifacts = useAppStore((s) => s.refreshAllArtifacts);
  const viewArtifact = useAppStore((s) => s.viewArtifact);
  const conversations = useAppStore((s) => s.conversations);

  useEffect(() => {
    refreshAllArtifacts();
  }, [refreshAllArtifacts]);

  const conversationTitles = useMemo(() => {
    const map: Record<string, string> = {};
    for (const c of conversations) map[c.id] = c.title;
    return map;
  }, [conversations]);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Library</h1>
        <p className="lede">Every artifact the assistant created, gathered in one place.</p>

        {allArtifacts.length === 0 ? (
          <div className="placeholder-note">No artifacts yet. Ask the assistant to create a document, web page, image, or diagram.</div>
        ) : (
          <div className="artifact-grid">
            {allArtifacts.map((a) => (
              <ArtifactCard
                key={a.id}
                artifact={a}
                conversationTitle={a.conversation_id ? conversationTitles[a.conversation_id] : undefined}
                onClick={() => viewArtifact(a)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ArtifactCard({
  artifact,
  conversationTitle,
  onClick,
}: {
  artifact: Artifact;
  conversationTitle?: string;
  onClick: () => void;
}) {
  return (
    <article className="artifact-card" tabIndex={0} onClick={onClick} onKeyDown={(e) => e.key === "Enter" && onClick()} aria-label={`Open ${artifact.title}`}>
      <div className="artifact-preview">
        <ArtifactPreview kind={artifact.kind} content={artifact.content} />
      </div>
      <div className="artifact-card-body">
        <div className="artifact-card-head">
          <h3 className="artifact-title" title={artifact.title}>{artifact.title}</h3>
          <span className="artifact-kind">{artifact.kind}</span>
        </div>
        <div className="artifact-card-meta">
          <span>{formatDate(artifact.created_at)}</span>
          {artifact.conversation_id && (
            <span className="artifact-source" title={conversationTitle}>
              from {conversationTitle || "a chat"}
            </span>
          )}
        </div>
      </div>
    </article>
  );
}

function ArtifactPreview({ kind, content }: { kind: string; content: string }) {
  if (kind === "image") return <ImagePreview path={content} />;
  if (kind === "html") return <iframe className="preview-frame" title="HTML preview" sandbox="" srcDoc={content} />;
  if (kind === "svg") {
    const doc = `<!doctype html><meta charset="utf8"><body style="margin:0;display:flex;align-items:center;justify-content:center;height:100%">${content}</body>`;
    return <iframe className="preview-frame" title="SVG preview" sandbox="" srcDoc={doc} />;
  }
  if (kind === "markdown") {
    return (
      <div className="preview-markdown">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{truncate(content, 220)}</ReactMarkdown>
      </div>
    );
  }
  return (
    <pre className="preview-code">
      <code>{truncate(content, 220)}</code>
    </pre>
  );
}

function ImagePreview({ path }: { path: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setSrc(null);
    setFailed(false);
    readImageDataUri(path)
      .then((uri) => !cancelled && setSrc(uri))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [path]);
  if (failed) return <div className="preview-empty">Image unavailable</div>;
  if (!src) return <div className="preview-empty">Loading image…</div>;
  return <img className="preview-image" src={src} alt="" />;
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + "…";
}

function formatDate(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}
