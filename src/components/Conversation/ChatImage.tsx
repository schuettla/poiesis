import { useEffect, useState } from "react";
import { readImageDataUri, inTauri } from "../../lib/api";

/** Renders an image message inline in the chat, loading it from disk by path
 * (generated images) or using an inline data URI. */
export default function ChatImage({ path, dataUri }: { path: string; dataUri?: string }) {
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

  if (failed) return <div className="chat-image-note">This image is no longer available.</div>;
  if (!src) return <div className="chat-image-note">Loading image…</div>;
  return <img className="chat-image" src={src} alt="Generated" />;
}
