import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useAppStore } from "../../lib/store";
import {
  contentVersion,
  inTauri,
  previewBaseUrl,
  readImageDataUri,
  readTextFile,
  runCodeArtifact,
  type CodeRun,
} from "../../lib/api";
import { languageForPath, languageForTitle, tokenize, useTokens, type ThemedToken } from "../../lib/highlight";
import { PlayIcon } from "../Icons/Icons";
import CodeEditor from "./CodeEditor";
import TokenLine from "./TokenLine";
import DiagnosticsList from "../Conversation/Diagnostics";
import {
  ConsolePanel,
  fixPrompt,
  instrument,
  usePreviewConsole,
  type ConsoleEntry,
} from "./PreviewConsole";

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
export function ArtifactView({
  kind,
  content,
  artifactId,
  title,
  path,
}: {
  kind: string;
  content: string;
  /** `ART-5`: present for a real artifact, absent for a file preview. Only an
   * artifact has somewhere for the agent to read its console back from. */
  artifactId?: string;
  /** Where the code's language comes from: an artifact's title, a file's path. */
  title?: string;
  path?: string;
}) {
  if (kind === "html") {
    return <HtmlView content={content} artifactId={artifactId} />;
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
  if (artifactId) {
    return <CodeArtifactView content={content} artifactId={artifactId} title={title} />;
  }
  return (
    <pre className="canvas-code">
      <Highlighted content={content} path={path} title={title} />
    </pre>
  );
}

/** The Shiki tokens for a piece of code, or null while they load or when the
 * language is unknown. The language comes from a path first, then a title. */
function useCodeTokens(content: string, path?: string, title?: string): ThemedToken[][] | null {
  return useTokens(async () => {
    const lang = path ? await languageForPath(path) : title ? await languageForTitle(title) : null;
    return lang ? tokenize(content, lang) : null;
  }, [content, path, title]);
}

/** Code in a `<code>` element: plain until its tokens land, then coloured.
 * The text is the same either way, so selecting and copying are unchanged. */
function Highlighted({ content, path, title }: { content: string; path?: string; title?: string }) {
  const tokens = useCodeTokens(content, path, title);
  if (!tokens) return <code>{content}</code>;
  return (
    <code className="hl">
      {tokens.map((line, i) => (
        <span key={i}>
          <TokenLine tokens={line} />
          {i < tokens.length - 1 ? "\n" : null}
        </span>
      ))}
    </code>
  );
}

/** `COD-UI-5`: a code artifact with a Run button and what it printed below the
 * source — the Canvas's own verification loop, styled as the HTML preview's
 * console so the two lanes read as one idea. A traceback shows in the same
 * diagnostics rows a project build does. */
function CodeArtifactView({ content, artifactId, title }: { content: string; artifactId: string; title?: string }) {
  const [run, setRun] = useState<CodeRun | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A revised artifact is different code: what the old one printed is stale.
  useEffect(() => {
    setRun(null);
    setError(null);
  }, [content]);

  const start = async () => {
    setRunning(true);
    setError(null);
    try {
      setRun(await runCodeArtifact(artifactId));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="canvas-html">
      <pre className="canvas-code viewer-code-scroll">
        <Highlighted content={content} title={title} />
      </pre>
      <div className="preview-console open">
        <div className="pc-head">
          {inTauri() && (
            <button className="pc-toggle viewer-run" onClick={start} disabled={running}>
              <PlayIcon size={12} />
              {running ? "Running…" : run ? "Run again" : "Run"}
            </button>
          )}
          {run && (
            <span className={`viewer-run-outcome ${run.exit_code === 0 ? "ok" : "failed"}`}>{run.outcome}</span>
          )}
        </div>
        {error && <p className="viewer-run-error">{error}</p>}
        {run && (
          <div className="pc-list viewer-run-body" role="log">
            <DiagnosticsList items={run.diagnostics} />
            {run.output && <pre className="viewer-run-output">{run.output}</pre>}
          </div>
        )}
      </div>
    </div>
  );
}

/** An HTML preview, plus what it printed (`ART-5`, `ART-6`).
 *
 * An artifact is *served* — from the loopback origin in `agent/preview.rs` —
 * rather than spliced into `srcdoc`. That is what gives the page a CSP of its
 * own (so it can load a font or a CDN script, which the app's own
 * `default-src 'self'` forbade), a real origin (so it gets `localStorage`, ES
 * modules and relative URLs), and a URL the agent's `check_preview` can open in
 * a real browser — the same URL, so the agent debugs the page the user is
 * looking at rather than a second rendering of it.
 *
 * A *file* preview has no artifact id and so no URL; it keeps the old inline
 * path, as does any session where the server didn't start. Either way the
 * console bridge is the same one, so this panel behaves identically.
 *
 * The sandbox keeps `allow-scripts`; served artifacts also get
 * `allow-same-origin`, which is safe precisely because the origin they are
 * same-origin *with* is the artifact server, not the app — the app's window and
 * the Tauri bridge stay out of reach either way. */
function HtmlView({ content, artifactId }: { content: string; artifactId?: string }) {
  const frame = useRef<HTMLIFrameElement | null>(null);
  const doc = useMemo(() => instrument(content), [content]);
  const [base, setBase] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    previewBaseUrl().then((b) => !cancelled && setBase(b));
    return () => {
      cancelled = true;
    };
  }, []);
  // The version is the content's, not a counter: an update in place keeps the
  // same id, and without a changed URL the frame would go on showing the page
  // the user just asked to have fixed.
  const src =
    base && artifactId ? `${base}/${artifactId}?v=${contentVersion(content)}` : null;
  const { entries, clear } = usePreviewConsole(frame, artifactId, content, src);
  const busy = useAppStore((s) => s.busy);
  const title = useAppStore((s) => {
    const convId = s.activeConversationId;
    if (!convId || !artifactId) return null;
    return s.artifacts[convId]?.find((a) => a.id === artifactId)?.title ?? null;
  });

  // Hands the error straight to the agent as a turn the user could have typed
  // themselves — the same path the composer uses, so it lands in the timeline
  // and answers in place rather than being some side channel.
  const onFix =
    artifactId && title
      ? (entry: ConsoleEntry) => {
          void useAppStore.getState().sendMessage(fixPrompt(entry, title, artifactId));
        }
      : undefined;

  return (
    <div className="canvas-html">
      {src ? (
        <iframe
          ref={frame}
          className="canvas-frame"
          title="HTML artifact"
          sandbox="allow-scripts allow-same-origin allow-forms"
          src={src}
        />
      ) : (
        <iframe
          ref={frame}
          className="canvas-frame"
          title="HTML artifact"
          sandbox="allow-scripts"
          srcDoc={doc}
        />
      )}
      <ConsolePanel entries={entries} onClear={clear} onFix={onFix} fixDisabled={busy} />
    </div>
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

/** Kinds that have something to *show* as well as something to edit, and so
 * get the Source/Preview toggle. Plain code has only a source, so it opens
 * straight into the editor with no toggle to read past. */
const PREVIEWABLE: RenderKind[] = ["markdown", "html", "svg"];

/** Does this file render as well as read? Exported because the toggle lives in
 * the pane's header (`ItemView`), which therefore has to know before it draws
 * the row whether there is anything to toggle between. */
export function canPreview(path: string): boolean {
  return PREVIEWABLE.includes(kindForPath(path));
}

/** Reads a file off disk and renders it by extension. Unlike an artifact, the
 * content isn't in memory — so this is the one view that can fail, and says so
 * plainly rather than showing an empty pane.
 *
 * `EDT-1`: a text file opens in a real editor rather than a `<pre>`. A file
 * that also *renders* — Markdown, HTML, SVG — still opens rendered, since that
 * is what you opened it to see, and `editing` switches it to its source.
 * Binaries and media have no source and keep their old views untouched.
 *
 * `editing` is owned by `ItemView`, because the control that sets it sits in
 * the pane's header alongside Save and Close rather than over the content. */
export function FileView({
  path,
  line,
  editing,
}: {
  path: string;
  line?: number;
  editing?: boolean;
}) {
  const conversationId = useAppStore((s) => s.activeConversationId);
  const kind = kindForPath(path);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The preview is a render of what is *on disk*; a save is the only thing
  // that changes it, so it is re-read whenever this file's dirty flag clears.
  const unsaved = useAppStore((s) => !!s.unsavedFiles[path]);
  const showPreview = PREVIEWABLE.includes(kind) && !editing;

  useEffect(() => {
    if (!showPreview) return;
    let cancelled = false;
    setText(null);
    setError(null);
    readTextFile(path, conversationId ?? undefined)
      .then((t) => !cancelled && setText(t))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [path, showPreview, unsaved, conversationId]);

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

  if (!showPreview) {
    return (
      <div className="editor-pane">
        <CodeEditor path={path} line={line} />
      </div>
    );
  }
  if (error) return <div className="canvas-loading">{error}</div>;
  if (text === null) return <div className="canvas-loading">Loading…</div>;
  // SVG files are markup: show them rendered, not as source. That and HTML are
  // iframes, which fill the row and scroll themselves; Markdown is flowed text
  // and needs a scroller around it, since the pane it now sits in is clipped
  // rather than scrolling the way `.wb-viewer-body` does everywhere else.
  const preview = <ArtifactView kind={kind === "svg" ? "svg" : kind} content={text} path={path} />;
  return (
    <div className="editor-pane">
      {kind === "markdown" ? <div className="editor-preview">{preview}</div> : preview}
    </div>
  );
}
