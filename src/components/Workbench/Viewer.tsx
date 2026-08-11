import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useAppStore } from "../../lib/store";
import { readImageDataUri, readTextFile } from "../../lib/api";

/** Render kinds the viewer knows. Artifacts carry theirs; files get one from
 * their extension — one renderer, two origins, which is the whole point of
 * putting files and artifacts in the same panel. */
export type RenderKind = "html" | "svg" | "image" | "video" | "markdown" | "code" | "binary";

const IMAGE_EXT = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico"];
// `ART-3`: these used to sit in `BINARY_EXT`, so a generated clip in a
// working folder rendered as "This file can't be previewed here" instead of
// playing.
const VIDEO_EXT = ["mp4", "mov", "webm", "mkv"];
const BINARY_EXT = [
  "pdf", "zip", "gz", "7z", "rar", "exe", "dll", "bin", "so", "dylib",
  "mp3", "wav", "avi", "ttf", "otf", "woff", "woff2",
  "doc", "docx", "xls", "xlsx", "ppt", "pptx", "gguf", "safetensors",
];

export function kindForPath(path: string): RenderKind {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "svg") return "svg";
  if (ext === "html" || ext === "htm") return "html";
  if (ext === "md" || ext === "markdown") return "markdown";
  if (IMAGE_EXT.includes(ext)) return "image";
  if (VIDEO_EXT.includes(ext)) return "video";
  if (BINARY_EXT.includes(ext)) return "binary";
  return "code";
}

/** Render an artifact or a file's contents. HTML and SVG go into a sandboxed
 * iframe with no same-origin access, so nothing rendered here can reach the
 * app's state or the Tauri bridge. */
export function ArtifactView({ kind, content }: { kind: string; content: string }) {
  if (kind === "html") {
    return (
      <iframe className="canvas-frame" title="HTML artifact" sandbox="allow-scripts" srcDoc={content} />
    );
  }
  if (kind === "svg") {
    const doc = `<!doctype html><meta charset="utf8"><body style="margin:0;display:flex;align-items:center;justify-content:center;height:100vh">${content}</body>`;
    return <iframe className="canvas-frame" title="SVG artifact" sandbox="" srcDoc={doc} />;
  }
  if (kind === "image") {
    return <ImageView path={content} />;
  }
  if (kind === "video") {
    return <VideoView path={content} />;
  }
  if (kind === "markdown") {
    return (
      <div className="canvas-markdown">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
      </div>
    );
  }
  return (
    <pre className="canvas-code">
      <code>{content}</code>
    </pre>
  );
}

/** Loads an image from disk (by path) as a data URI for display. */
export function ImageView({ path }: { path: string }) {
  const conversationId = useAppStore((s) => s.activeConversationId);
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setSrc(null);
    setFailed(false);
    readImageDataUri(path, conversationId ?? undefined)
      .then((uri) => !cancelled && setSrc(uri))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [path, conversationId]);
  if (failed) return <div className="canvas-loading">This image is no longer available.</div>;
  if (!src) return <div className="canvas-loading">Loading image…</div>;
  return <img className="canvas-image" src={src} alt="" />;
}

/** A generated clip, played straight from disk via the asset protocol —
 * never base64'd over IPC, which a video-sized payload would make painfully
 * slow (`ART-3`). */
export function VideoView({ path }: { path: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <div className="canvas-loading">This video is no longer available.</div>;
  return (
    <video
      className="canvas-image"
      src={convertFileSrc(path)}
      controls
      loop
      muted
      playsInline
      preload="metadata"
      onError={() => setFailed(true)}
    />
  );
}

/** Reads a file off disk and renders it by extension. Unlike an artifact, the
 * content isn't in memory — so this is the one view that can fail, and says so
 * plainly rather than showing an empty pane. */
export function FileView({ path }: { path: string }) {
  const conversationId = useAppStore((s) => s.activeConversationId);
  const kind = kindForPath(path);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (kind === "image" || kind === "video" || kind === "binary") return;
    let cancelled = false;
    setText(null);
    setError(null);
    readTextFile(path, conversationId ?? undefined)
      .then((t) => !cancelled && setText(t))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [path, kind, conversationId]);

  if (kind === "image") return <ImageView path={path} />;
  if (kind === "video") return <VideoView path={path} />;
  if (kind === "binary") {
    return (
      <div className="viewer-binary">
        <p>This file can't be previewed here.</p>
        <button className="wb-link" onClick={() => useAppStore.getState().openInSystem(path)}>
          Open with the default app
        </button>
      </div>
    );
  }
  if (error) return <div className="canvas-loading">{error}</div>;
  if (text === null) return <div className="canvas-loading">Loading…</div>;
  // SVG files are markup: show them rendered, not as source.
  return <ArtifactView kind={kind === "svg" ? "svg" : kind} content={text} />;
}
