import { inTauri, saveArtifactFile, type Artifact } from "../../lib/api";
import { save } from "@tauri-apps/plugin-dialog";

/** Best-guess file extension for an artifact, by kind. */
export function extFor(artifact: Artifact): string {
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
