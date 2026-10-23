import { useState } from "react";
import "./ComposioApps.css";

/**
 * An app's mark. The marks ship with the app under `/app-logos/` (the webview
 * loads no remote images); an app without one, or one that fails to load, gets
 * its first letter instead.
 */
export default function AppLogo({
  slug,
  name,
  src,
  large,
}: {
  slug: string;
  name: string;
  /** Where the mark is, `null` for none. Omitted: the bundled path for `slug`. */
  src?: string | null;
  large?: boolean;
}) {
  const [broken, setBroken] = useState(false);
  const url = src === undefined ? `/app-logos/${slug}.svg` : src;
  return (
    <span className={`app-logo ${large ? "large" : ""}`} aria-hidden="true">
      {url && !broken ? (
        <img src={url} alt="" loading="lazy" draggable={false} onError={() => setBroken(true)} />
      ) : (
        name.trim().charAt(0).toUpperCase() || "?"
      )}
    </span>
  );
}
