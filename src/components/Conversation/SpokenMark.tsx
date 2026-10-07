import { useState } from "react";
import type { Message } from "../../lib/types";
import { WaveformIcon } from "../Icons/Icons";
import "./SpokenMark.css";

/** A small wave beside the time: this turn was spoken (`VOC-UI-5`). */
export function SpokenGlyph() {
  return (
    <span className="spoken-glyph" role="img" aria-label="Spoken" title="Spoken">
      <WaveformIcon size={11} />
    </span>
  );
}

/**
 * Under a reply the user cut off: what is shown is only what they heard, and
 * this says so, with the whole reply one click away (`VTN-6`, `VXP-5`).
 */
export function StoppedHere({ message }: { message: Message }) {
  const [open, setOpen] = useState(false);
  if (message.stopReason !== "interrupted") return null;
  return (
    <div className="spoken-stop">
      <span className="spoken-stop-mark">Stopped here</span>
      {message.unspoken && (
        <button className="why-link" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          {open ? "Hide the full reply" : "Show the full reply"}
        </button>
      )}
      {open && message.unspoken && <p className="spoken-full">{message.unspoken}</p>}
    </div>
  );
}
