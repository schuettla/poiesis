import { inTauri, saveArtifactFile, type Artifact } from "../../lib/api";
import { save } from "@tauri-apps/plugin-dialog";
import type { ComponentType } from "react";
import { CodeIcon, DocumentIcon, FileIcon, GlobeIcon, ImageIcon, VideoIcon, type IconProps } from "../Icons/Icons";

/** Best-guess file extension for an artifact, by kind. */
export function extFor(artifact: Pick<Artifact, "kind" | "content">): string {
  // `ART-3`: a video artifact's `content` is a path too, same as an image's —
  // the real extension is already sitting right there.
  if (artifact.kind === "image") return artifact.content.split(".").pop()?.toLowerCase() || "png";
  if (artifact.kind === "video") return artifact.content.split(".").pop()?.toLowerCase() || "mp4";
  switch (artifact.kind) {
    case "html":
      return "html";
    case "svg":
      return "svg";
    case "markdown":
      return "md";
    default:
      return "txt";
  }
}

/** What an artifact is, in words and as a drawing: the message stream's card and
 * the Workbench's list both say "Document", not the raw kind string. */
const KINDS: Record<string, { label: string; Icon: ComponentType<IconProps> }> = {
  markdown: { label: "Document", Icon: DocumentIcon },
  html: { label: "Web page", Icon: GlobeIcon },
  svg: { label: "Drawing", Icon: ImageIcon },
  image: { label: "Image", Icon: ImageIcon },
  video: { label: "Video", Icon: VideoIcon },
  code: { label: "Code", Icon: CodeIcon },
};

export function artifactKind(artifact: Artifact): { label: string; Icon: ComponentType<IconProps>; meta: string } {
  const { label, Icon } = KINDS[artifact.kind] ?? { label: artifact.kind, Icon: FileIcon };
  return { label, Icon, meta: artifact.kind === "code" ? label : `${label} · ${extFor(artifact).toUpperCase()}` };
}

export function slugify(title: string): string {
  return (
    title
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/(^-|-$)/g, "") || "artifact"
  );
}

/** Save an artifact anywhere on disk via the native dialog. The escape hatch
 * for when there's no working folder to write it into. */
export async function downloadArtifact(artifact: Artifact) {
  const ext = extFor(artifact);
  if (!inTauri()) {
    if (artifact.kind === "image" || artifact.kind === "video") return;
    const blob = new Blob([artifact.content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${slugify(artifact.title)}.${ext}`;
    a.click();
    URL.revokeObjectURL(url);
    return;
  }
  const dest = await save({ defaultPath: `${slugify(artifact.title)}.${ext}` });
  if (!dest) return;
  await saveArtifactFile(dest, artifact.kind, artifact.content);
}
