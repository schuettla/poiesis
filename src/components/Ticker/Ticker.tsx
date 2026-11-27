import { useEffect, useState } from "react";
import "./Ticker.css";

const SLIDE_MS = 220;

/** `UI-2`/`UI-4`: a line of words that changes in place. The old words slide up and fade out,
 * the new ones come in from below, both in `--dur`. Under reduced motion it just changes
 * (the global rule in `tokens.css` sets every animation to 0s). It sizes itself to one line
 * of whatever font it sits in, and clips to it. `live` passes the moving light through the
 * words (`.live-words`), for the line that says what is happening now. With `quiet` the first
 * words are simply there, and only a change after that slides. */
export default function Ticker({
  text,
  live = false,
  quiet = false,
}: {
  text: string;
  live?: boolean;
  quiet?: boolean;
}) {
  const [shown, setShown] = useState(text);
  const [out, setOut] = useState<string | null>(null);
  const [still, setStill] = useState(quiet);

  useEffect(() => {
    if (text === shown) return;
    setOut(shown);
    setShown(text);
    setStill(false);
    const t = setTimeout(() => setOut(null), SLIDE_MS);
    return () => clearTimeout(t);
    // `shown` is what this effect sets; only a change of `text` runs it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);

  return (
    <span className="tick">
      {out !== null && (
        <span className="tick-text out" aria-hidden="true">
          {out}
        </span>
      )}
      <span className={still ? "tick-text" : "tick-text in"} key={shown}>
        {live ? <span className="live-words">{shown}</span> : shown}
      </span>
    </span>
  );
}
