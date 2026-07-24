import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useAppStore } from "../../lib/store";
import { readImageDataUri } from "../../lib/api";
import "./CanvasPanel.css";

/** The Canvas side panel (CHT-6): renders model-produced artifacts. HTML and SVG
 * are shown in a sandboxed iframe with no same-origin access, so an artifact can
 * never reach the app's state or the Tauri bridge. */
export default function CanvasPanel() {
  const canvasOpen = useAppStore((s) => s.canvasOpen);
  const closeCanvas = useAppStore((s) => s.closeCanvas);
  const activeConversationId = useAppStore((s) => s.activeConversationId);
  // Select stable slices only — deriving `artifacts` outside the selector avoids
  // returning a fresh array each call (which infinite-loops in Zustand v5).
  const artifactsMap = useAppStore((s) => s.artifacts);
  const activeArtifactId = useAppStore((s) => s.activeArtifactId);
  const openCanvas = useAppStore((s) => s.openCanvas);

  const artifacts = activeConversationId ? artifactsMap[activeConversationId] ?? [] : [];

  if (!canvasOpen || !activeConversationId || artifacts.length === 0) return null;

  const active = artifacts.find((a) => a.id === activeArtifactId) ?? artifacts[artifacts.length - 1];

  return (
    <aside className="canvas-panel" aria-label="Canvas">
      <div className="canvas-head">
        <div className="canvas-title-wrap">
          {artifacts.length > 1 ? (
            <select
              className="canvas-select"
              value={active.id}
              onChange={(e) => openCanvas(e.target.value)}
              aria-label="Choose an artifact"
            >
              {artifacts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.title}
                </option>
              ))}
            </select>
          ) : (
            <span className="canvas-title">{active.title}</span>
          )}
          <span className="canvas-kind">{active.kind}</span>
        </div>
        <button className="canvas-close" aria-label="Close Canvas" onClick={closeCanvas}>
          ×
        </button>
      </div>

      <div className="canvas-body">
        <ArtifactView kind={active.kind} content={active.content} />
      </div>
    </aside>
  );
}

function ArtifactView({ kind, content }: { kind: string; content: string }) {
  if (kind === "html") {
    return (
      <iframe
        className="canvas-frame"
        title="HTML artifact"
        sandbox="allow-scripts"
        srcDoc={content}
      />
    );
  }
  if (kind === "svg") {
    const doc = `<!doctype html><meta charset="utf8"><body style="margin:0;display:flex;align-items:center;justify-content:center;height:100vh">${content}</body>`;
    return <iframe className="canvas-frame" title="SVG artifact" sandbox="" srcDoc={doc} />;
  }
  if (kind === "image") {
    return <ImageArtifact path={content} />;
  }
  if (kind === "markdown") {
    return (
      <div className="canvas-markdown">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
      </div>
    );
  }
  // code (and any unknown kind): show as monospace source.
  return (
    <pre className="canvas-code">
      <code>{content}</code>
    </pre>
  );
}

/** Loads a generated PNG from disk (by path) as a data URI for display (9F). */
function ImageArtifact({ path }: { path: string }) {
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
  if (failed) return <div className="canvas-loading">This image is no longer available.</div>;
  if (!src) return <div className="canvas-loading">Loading image…</div>;
  return <img className="canvas-image" src={src} alt="Generated" />;
}
