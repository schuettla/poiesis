import { useEffect, useState } from "react";
import { readImageDataUri, inTauri } from "../../lib/api";

/** Loads an image from disk by path (or an inline data URI) and renders it —
 * the bare loading primitive `ChatMedia` builds its media block on top of.
 * Also used directly wherever a picture just needs to be shown with no
 * artifact behind it, e.g. a live browser screenshot. */
export default function ImageByPath({
  path,
  dataUri,
  alt,
  className,
}: {
  path: string;
  dataUri?: string;
  alt?: string;
  className?: string;
}) {
  const [src, setSrc] = useState<string | null>(dataUri ?? null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (dataUri) {
      setSrc(dataUri);
      return;
    }
    if (!inTauri() || !path) return;
    let cancelled = false;
    readImageDataUri(path)
      .then((uri) => !cancelled && setSrc(uri))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [path, dataUri]);

  if (failed) return <div className="chat-media-note">This image is no longer available.</div>;
  if (!src) return <div className="chat-media-note">Loading image…</div>;
  return <img className={className ?? "chat-media-el"} src={src} alt={alt ?? "Image"} />;
}
