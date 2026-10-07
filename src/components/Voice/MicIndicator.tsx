import { useMicOpen, leaveVoice } from "../../lib/voice/controller";
import { MicIcon } from "../Icons/Icons";

/** Shown in the top bar whenever the voice conversation has the mic open, even
 * when its surface is hidden (`VOC-UI-10`, `VXP-3`). Pressing it closes the mic. */
export default function MicIndicator() {
  const open = useMicOpen();
  if (!open) return null;
  return (
    <button
      className="sidebar-toggle voice-mic-open"
      onClick={() => void leaveVoice()}
      aria-label="The microphone is on. End the voice conversation"
      title="Listening. Click to end the voice conversation."
    >
      <MicIcon size={16} />
      <span className="voice-mic-dot" aria-hidden="true" />
    </button>
  );
}
