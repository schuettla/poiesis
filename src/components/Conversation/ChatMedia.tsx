import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readImageDataUri, inTauri } from "../../lib/api";
import { parseArtifactMeta, useAppStore } from "../../lib/store";
import type { Attachment } from "../../lib/types";
import { downloadArtifact } from "../Workbench/artifactFiles";

/** Holds the final aspect ratio while a generation is in flight, so the
 * transcript never reflows when the media lands (`STR-2`). The elapsed counter
 * appears after 3s — before that the wait reads as instant and a timer would
 * only draw attention to it — and Cancel after 10s, by which point waiting has
 * become a decision rather than a moment. */
export function ChatMediaPending({
  modality,
  aspectRatio,
  startedAt,
  jobId,
}: {
  modality: "image" | "video";
  aspectRatio?: string;
  startedAt: number;
  jobId?: string;
}) {
  const cancelMediaJob = useAppStore((s) => s.cancelMediaJob);
  const partial = useAppStore((s) => (jobId ? s.mediaPartials[jobId] : undefined));
  const [elapsed, setElapsed] = useState(() => Math.floor((Date.now() - startedAt) / 1000));

  useEffect(() => {
    const t = setInterval(() => setElapsed(Math.floor((Date.now() - startedAt) / 1000)), 1000);
    return () => clearInterval(t);
  }, [startedAt]);

  // "16:9" is a CSS aspect-ratio once the colon becomes a slash; video with no
  // stated ratio is 16:9 rather than square, which is what every model returns.
  const ratio = aspectRatio?.replace(":", " / ") ?? (modality === "video" ? "16 / 9" : "1 / 1");

  return (
    <div
      className="chat-media-skeleton"
      style={{ aspectRatio: ratio }}
      role="img"
      aria-label={modality === "video" ? "Generating a video…" : "Generating an image…"}
    >
      {/* `STR-4`: once partials are arriving, the picture resolving in place
          is a better progress indicator than any animation, so the shimmer
          gives way to it. */}
      {partial ? (
        <img className="chat-media-partial" src={partial} alt="" aria-hidden="true" />
      ) : (
        <div className="chat-media-shimmer" aria-hidden="true" />
      )}
      {elapsed >= 3 && (
        <div className="chat-media-waiting">
          <span className="chat-media-elapsed">
            {Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, "0")}
          </span>
          {elapsed >= 10 && jobId && (
            <button className="chat-media-cancel" onClick={() => cancelMediaJob(jobId)}>
              Cancel
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** Renders generated (or attached) media inline in the chat — the media block
 * (`STR-2`). Images are read as a data URI; video goes through the asset
 * protocol instead, because base64-ing a 20 MB MP4 over IPC would freeze the
 * transcript. Once an artifact backs the attachment, a quiet action row offers
 * Refine / Variation / Save / download / open underneath it. */
export default function ChatMedia({ attachment, alt }: { attachment: Attachment; alt?: string }) {
  const { kind, path, dataUri, artifactId, width, height } = attachment;
  const isVideo = kind === "video";
  const [src, setSrc] = useState<string | null>(dataUri ?? null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (dataUri) {
      setSrc(dataUri);
      return;
    }
    if (!inTauri() || !path) return;
    if (isVideo) {
      setSrc(convertFileSrc(path));
      return;
    }
    let cancelled = false;
    readImageDataUri(path)
      .then((uri) => !cancelled && setSrc(uri))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [path, dataUri, isVideo]);

  const activeConversationId = useAppStore((s) => s.activeConversationId);
  const artifactsByConv = useAppStore((s) => (activeConversationId ? s.artifacts[activeConversationId] : undefined));
  const conv = useAppStore((s) => s.conversations.find((c) => c.id === activeConversationId));
  const mediaModels = useAppStore((s) => s.mediaModels);
  const saveArtifactToFolder = useAppStore((s) => s.saveArtifactToFolder);
  const viewArtifact = useAppStore((s) => s.viewArtifact);
  const createImage = useAppStore((s) => s.createImage);
  const refineArtifact = useAppStore((s) => s.refineArtifact);

  const [saving, setSaving] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const artifact = artifactId ? artifactsByConv?.find((a) => a.id === artifactId) : undefined;
  const meta = parseArtifactMeta(artifact?.meta_json);
  const providerLabel = typeof meta.provider_label === "string" ? meta.provider_label : undefined;
  const costUsd = typeof meta.cost_usd === "number" ? meta.cost_usd : undefined;
  const ignored = Array.isArray(meta.ignored) ? (meta.ignored as string[]) : [];
  const prompt = typeof meta.prompt === "string" ? meta.prompt : undefined;
  const canSave = !!conv?.folderPath && !artifact?.saved_path;

  // Refine only exists where the producing model can actually take the old
  // image as input. `model_id` is `<backend>:<slug>`; the media model list
  // carries the `supports_edit` the backend reported for it (`STR-2`).
  const modelId = typeof meta.model_id === "string" ? meta.model_id : undefined;
  const producedBy = modelId
    ? mediaModels.find((m) => m.id === `media:${modelId.replace(":", "/")}`)
    : undefined;
  const canRefine = !isVideo && !!prompt && !!artifact && (producedBy?.supports_edit ?? false);

  // The attachment carries dimensions while the turn is live; after a reload
  // only the artifact's metadata has them, and it is the same two numbers.
  const w = width ?? (typeof meta.width === "number" ? meta.width : undefined);
  const h = height ?? (typeof meta.height === "number" ? meta.height : undefined);
  const ratio = w && h ? `${w} / ${h}` : isVideo ? "16 / 9" : "1 / 1";

  if (failed) {
    return <div className="chat-media-note">This {isVideo ? "video" : "image"} is no longer available.</div>;
  }
  if (!src) {
    return (
      <div className="chat-media-skeleton" style={{ aspectRatio: ratio }} role="img" aria-label="Loading…">
        <div className="chat-media-shimmer" aria-hidden="true" />
      </div>
    );
  }

  return (
    <figure className="chat-media">
      {isVideo ? (
        <video
          className="chat-media-el"
          src={src}
          controls
          loop
          muted
          playsInline
          preload="metadata"
          onError={() => setFailed(true)}
        />
      ) : (
        <img className="chat-media-el" src={src} alt={alt ?? prompt ?? "Generated image"} />
      )}
      {artifact && (
        <div className="chat-media-actions">
          {canRefine && (
            <button
              className="chat-media-action"
              title="Describe a change to this image"
              onClick={() => refineArtifact(artifact)}
            >
              Refine
            </button>
          )}
          {prompt && !isVideo && (
            <button
              className="chat-media-action"
              title="Generate another take on the same prompt"
              onClick={() => createImage(prompt)}
            >
              Variation
            </button>
          )}
          {canSave && (
            <button
              className="chat-media-action"
              disabled={saving}
              onClick={async () => {
                setSaving(true);
                setActionError(null);
                try {
                  const ext = artifact.content.split(".").pop() || (isVideo ? "mp4" : "png");
                  const name = artifact.title.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-") || "image";
                  await saveArtifactToFolder(artifact.id, `${name}.${ext}`);
                } catch (e) {
                  setActionError(String(e));
                } finally {
                  setSaving(false);
                }
              }}
            >
              Save
            </button>
          )}
          <button className="chat-media-action" title="Download" onClick={() => downloadArtifact(artifact)}>
            ⤓
          </button>
          <button className="chat-media-action" title="Open in Workbench" onClick={() => viewArtifact(artifact)}>
            ↗
          </button>
        </div>
      )}
      {(providerLabel || w) && (
        <figcaption className="chat-media-meta">
          {[providerLabel, w && h ? `${w}×${h}` : null, costUsd != null ? `$${costUsd.toFixed(2)}` : null]
            .filter(Boolean)
            .join(" · ")}
          {ignored.length > 0 && <span> · {ignored[0]} wasn't available</span>}
        </figcaption>
      )}
      {actionError && <p className="chat-media-error">{actionError}</p>}
    </figure>
  );
}
