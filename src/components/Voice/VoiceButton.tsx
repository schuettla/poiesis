import { openVoice } from "../../lib/voice/controller";
import { useVoiceStore } from "../../lib/voice/voiceStore";
import { WaveformIcon } from "../Icons/Icons";

/**
 * The way back into a voice conversation that is still running (`VOC-UI-13`).
 * It exists only while one is live: "Show chat" hides the voice surface and keeps
 * the session, and this brings the surface back in one click. With no live
 * conversation it is not there; starting one is `/talk`, the shortcut or the
 * palette. The red dot is the same "mic is open" dot as everywhere (`VXP-3`).
 */
export default function VoiceButton() {
  const live = useVoiceStore((s) => s.floor !== "off");
  const shown = useVoiceStore((s) => s.shown);
  // The surface is already up, or there is no conversation to go back to.
  if (!live || shown) return null;
  return (
    <button
      className="icon-btn voice-return on"
      aria-label="Back to the voice conversation"
      title="Back to voice. The microphone is on."
      onClick={() => void openVoice()}
    >
      <WaveformIcon size={16} />
      <span className="mic-dot" aria-hidden="true" />
    </button>
  );
}
