import { useEffect, useState } from "react";
import { voiceDownload } from "../../lib/api";
import { voiceNeeds, type VoiceNeeds } from "../../lib/voice/controller";

/** `VOC-UI-7`: the optional step "Want to talk to me?". One button gets hearing
 * and a voice; it is never needed for anything else. Drawn as a list item for
 * the first-steps screen's "Later" list. */
export default function VoiceStep() {
  const [needs, setNeeds] = useState<VoiceNeeds | null | undefined>(undefined);
  const [percent, setPercent] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    voiceNeeds()
      .then(setNeeds)
      .catch(() => setNeeds(undefined));
  }, []);

  async function get() {
    if (!needs) return;
    setBusy(true);
    setFailed(false);
    try {
      const progress = (p: { received: number; total: number | null }) =>
        setPercent(p.total ? Math.round((p.received / p.total) * 100) : null);
      if (needs.hearingId) await voiceDownload("hearing", needs.hearingId, progress);
      if (needs.voiceModelId) await voiceDownload("voice", needs.voiceModelId, progress);
      setNeeds(null);
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }

  // Unknown (not the desktop app, or the check failed): say nothing.
  if (needs === undefined) return null;
  const done = needs === null;
  const size = needs ? [needs.hearingId && needs.hearingSize, needs.voiceModelId && needs.voiceSize].filter(Boolean).join(" and ") : "";
  return (
    <li className={done ? "done" : ""}>
      <span className="fs-mark" aria-hidden="true">
        {done ? "✓" : "+"}
      </span>
      <span className="fs-body">
        <strong>Talk to me out loud</strong>
        <span>
          {done
            ? "Ready. Press the wave button next to the message box."
            : "I listen and speak on your computer. Nothing is sent anywhere."}
        </span>
        {failed && <span>The download did not finish. Try again.</span>}
        {!done && (
          <button className="fs-link" onClick={get} disabled={busy}>
            {busy ? `Downloading${percent !== null ? ` ${percent}%` : "…"}` : `Get voice (${size.toLowerCase()}) →`}
          </button>
        )}
      </span>
    </li>
  );
}
