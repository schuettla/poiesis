import type { Photo } from "./store";

/** The longest side of a photo sent to the desktop, in pixels. A phone camera photo is far larger than a model needs. */
const MAX_SIDE = 1600;

/** Read a picked file as a JPEG data URI, scaled down. Rejects with a plain sentence. */
export async function readPhoto(file: File): Promise<Photo> {
  if (!file.type.startsWith("image/")) throw new Error("That is not a picture.");
  const bitmap = await createImageBitmap(file).catch(() => null);
  if (!bitmap) throw new Error("I could not open that picture.");
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("I could not open that picture.");
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close?.();
  return { name: file.name.replace(/\.[^.]+$/, "") + ".jpg", dataUri: canvas.toDataURL("image/jpeg", 0.85) };
}
