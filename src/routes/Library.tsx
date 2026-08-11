import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useAppStore } from "../lib/store";
import * as api from "../lib/api";
import { inTauri, type Artifact } from "../lib/api";
import "./Surface.css";
import "./Library.css";

/** `ART-3`'s filter row. "All" always first; the rest only when relevant. */
type LibraryFilter = "all" | "image" | "video" | "docs";
const DOC_KINDS = new Set(["html", "svg", "markdown", "code"]);

/** "$2.40 · 61 images, 3 clips" — the plan's own phrasing (`CST-2`). Counts
 * are stated even when a generation was free, because "what did I make" is
 * half the question the number answers. */
function formatSpend(spend: api.MediaSpend): string {
  const parts: string[] = [];
  if (spend.images > 0) parts.push(`${spend.images} image${spend.images === 1 ? "" : "s"}`);
  if (spend.videos > 0) parts.push(`${spend.videos} clip${spend.videos === 1 ? "" : "s"}`);
  const counts = parts.join(", ");
  return counts ? `$${spend.usd.toFixed(2)} · ${counts}` : `$${spend.usd.toFixed(2)}`;
}

export default function Library() {
  const allArtifacts = useAppStore((s) => s.allArtifacts);
  const refreshAllArtifacts = useAppStore((s) => s.refreshAllArtifacts);
  const viewArtifact = useAppStore((s) => s.viewArtifact);
  const conversations = useAppStore((s) => s.conversations);
  const [filter, setFilter] = useState<LibraryFilter>("all");

  useEffect(() => {
    refreshAllArtifacts();
  }, [refreshAllArtifacts]);

  const conversationTitles = useMemo(() => {
    const map: Record<string, string> = {};
    for (const c of conversations) map[c.id] = c.title;
    return map;
  }, [conversations]);

  const filtered = useMemo(() => {
    if (filter === "all") return allArtifacts;
    if (filter === "docs") return allArtifacts.filter((a) => DOC_KINDS.has(a.kind));
    return allArtifacts.filter((a) => a.kind === filter);
  }, [allArtifacts, filter]);

  const hasMedia = allArtifacts.some((a) => a.kind === "image" || a.kind === "video");

  // `CST-2`. Re-read whenever the library changes, since a new generation is
  // exactly the event that moves the number.
  const [spend, setSpend] = useState<api.MediaSpendReport | null>(null);
  useEffect(() => {
    if (!inTauri()) return;
    api.mediaSpend().then(setSpend).catch(() => setSpend(null));
  }, [allArtifacts.length]);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Library</h1>
        <p className="lede">Every artifact the assistant created, gathered in one place.</p>

        {hasMedia && (
          <div className="library-filter-row">
            {(["all", "image", "video", "docs"] as const).map((f) => (
              <button
                key={f}
                className={`filter-chip ${filter === f ? "active" : ""}`}
                aria-pressed={filter === f}
                onClick={() => setFilter(f)}
              >
                {f === "all" ? "All" : f === "image" ? "Images" : f === "video" ? "Video" : "Documents"}
              </button>
            ))}
            {/* `CST-2`: the running total, next to the thing it is a total of.
                Only when there is money to report — a purely local library
                says nothing about cost, which is itself the argument for
                local. */}
            {spend && spend.month.usd > 0 && (
              <span className="library-spend">
                {formatSpend(spend.month)} this month
              </span>
            )}
          </div>
        )}

        {filtered.length === 0 ? (
          <div className="placeholder-note">
            {allArtifacts.length === 0
              ? "No artifacts yet. Ask the assistant to create a document, web page, image, or diagram."
              : "Nothing in this filter yet."}
          </div>
        ) : (
          <div className="artifact-grid">
            {filtered.map((a) => (
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

function providerLabelFor(artifact: Artifact): string | undefined {
  if (!artifact.meta_json) return undefined;
  try {
    const meta = JSON.parse(artifact.meta_json) as Record<string, unknown>;
    return typeof meta.provider_label === "string" ? meta.provider_label : undefined;
  } catch {
    return undefined;
  }
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
  const providerLabel = providerLabelFor(artifact);
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
          {/* `ART-3`: which engine made it is the single most useful thing to
              know about a picture later — Nano Banana Pro vs local SDXL. */}
          {providerLabel && <span className="artifact-provider">{providerLabel}</span>}
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
  if (kind === "video") return <VideoPreview path={content} />;
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

// `ART-3`: a grid of thumbnails base64'd over IPC one-by-one is fine for a
// single 1.5 MB PNG but not for a shelf of them — the asset protocol streams
// the file directly, scoped to `generated_media_dir()` in `tauri.conf.json`.
function ImagePreview({ path }: { path: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <div className="preview-empty">Image unavailable</div>;
  return (
    <img
      className="preview-image"
      src={convertFileSrc(path)}
      alt=""
      onError={() => setFailed(true)}
    />
  );
}

function VideoPreview({ path }: { path: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <div className="preview-empty">Video unavailable</div>;
  return (
    <video
      className="preview-video"
      src={convertFileSrc(path)}
      muted
      loop
      playsInline
      preload="metadata"
      onError={() => setFailed(true)}
    />
  );
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + "…";
}

function formatDate(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}
