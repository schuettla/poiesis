import { useEffect } from "react";
import { useAppStore } from "../../lib/store";
import ImageByPath from "./ImageByPath";
import "./ImageLightbox.css";

/** Full-size view for a picture that isn't necessarily an artifact — a
 * pasted screenshot has no Workbench row to open (`STR-3`). */
export default function ImageLightbox() {
  const target = useAppStore((s) => s.imageLightbox);
  const close = useAppStore((s) => s.closeImageLightbox);

  useEffect(() => {
    if (!target) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [target, close]);

  if (!target) return null;
  return (
    <div className="image-lightbox-backdrop" onClick={close}>
      <button className="image-lightbox-close" aria-label="Close" onClick={close}>
        ×
      </button>
      <ImageByPath
        path={target.path ?? ""}
        dataUri={target.dataUri}
        alt={target.alt}
        className="image-lightbox-img"
      />
    </div>
  );
}
