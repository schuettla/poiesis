import { inTauri, saveArtifactFile, type Artifact } from "../../lib/api";
import { save } from "@tauri-apps/plugin-dialog";

/** Best-guess file extension for an artifact, by kind. */
export function extFor(artifact: Artifact): string {
  if (artifact.kind === "image") return artifact.content.split(".").pop()?.toLowerCase() || "png";
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
    if (artifact.kind === "image") return;
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
