import { useAppStore } from "../../lib/store";
import { openVoice } from "../../lib/voice/controller";
import { useVoiceStore } from "../../lib/voice/voiceStore";
import { WaveformIcon } from "../Icons/Icons";
import MicButton from "./MicButton";

/**
 * Voice, under the composer on the left: two buttons, both drawn icons.
 *
 * The mic dictates into the box (tap), or talks and sends (hold). The waveform
 * opens voice mode, the back-and-forth conversation that `/talk` also starts.
 * While a voice conversation is live but hidden, the same waveform carries the
 * red "mic is open" dot (`VXP-3`) and takes you back to it (`VOC-UI-13`).
 */
export default function VoiceControl(props: React.ComponentProps<typeof MicButton>) {
  const workspaceMode = useAppStore((s) => s.workspaceMode && s.workspaceShown);
  const live = useVoiceStore((s) => s.floor !== "off");
  const shown = useVoiceStore((s) => s.shown);
  const back = live && !shown;
  const title = back
    ? "Back to voice. The microphone is on."
    : workspaceMode
      ? "Voice mode: talk back and forth. Opens in the chat view, the workspace waits."
      : "Voice mode: talk back and forth, I answer out loud";

  return (
    <>
      <MicButton {...props} />
      <button
        className={`icon-btn voice-mode-btn${back ? " on" : ""}`}
        aria-label={back ? "Back to the voice conversation" : "Voice mode"}
        title={title}
        onClick={() => void openVoice()}
      >
        <WaveformIcon size={16} />
        {back && <span className="mic-dot" aria-hidden="true" />}
      </button>
    </>
  );
}
