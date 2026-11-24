import { leaveVoice, openVoice, voiceSession } from "../voice/controller";
import { setVoiceUi, useVoiceStore } from "../voice/voiceStore";
import { MicIcon, MicOffIcon, StopIcon } from "./Icons";
import Mark from "./Mark";

/** Who has the floor, in one or two words. Same words as the desktop's voice surface. */
export function stateLine(floor: string, muted: boolean): string {
  if (muted) return "Mic is muted";
  switch (floor) {
    case "starting":
    case "off":
      return "Getting ready";
    case "user_speaking":
    case "listening":
      return "Listening";
    case "thinking":
      return "Thinking";
    case "speaking":
      return "Speaking";
    default:
      return "";
  }
}

/** The last words heard, cut from the front so the newest stay in view. */
function tail(text: string, max = 90): string {
  return text.length > max ? `... ${text.slice(-max).trimStart()}` : text;
}

/**
 * Voice inside the chat: the bar takes the composer's place. What you say lands in the chat as your message, the
 * answer arrives there as text and is spoken, and questions show in the chat where you tap them. The mark grows with
 * the voice that has the floor. The bar never changes height, so nothing above it jumps.
 */
export default function VoiceBar() {
  const { floor, input, output, user, muted, hint, error, problem } = useVoiceStore();
  const level = Math.min(1, (floor === "speaking" ? output : input) * 6);
  const busy = floor === "thinking" || floor === "speaking";

  if (error) {
    return (
      <footer className="composer-dock">
        <div className="composer voice-bar" role="region" aria-label="Voice">
          <span className="voice-mark">
            <Mark state="away" size={30} />
          </span>
          <p className="voice-error" role="alert">
            {error}
          </p>
          <div className="voice-controls">
            <button className="btn btn-secondary btn-sm" onClick={() => void leaveVoice()}>
              Close
            </button>
            <button className="btn btn-primary btn-sm" onClick={() => void openVoice()}>
              Try again
            </button>
          </div>
        </div>
      </footer>
    );
  }

  const note = problem ?? hint;
  return (
    <footer className="composer-dock">
      {note && (
        <button className="composer-reason voice-note" onClick={() => setVoiceUi({ problem: null, hint: null })}>
          {note}
        </button>
      )}
      <div className="composer voice-bar" role="region" aria-label="Voice conversation">
        <span className="voice-mark" style={{ transform: `scale(${1 + level * 0.35})` }}>
          <Mark state={busy ? "working" : "resting"} size={30} />
        </span>
        <div className="voice-text">
          <p className="voice-state" aria-live="polite">
            {floor !== "off" && floor !== "starting" && !muted && <span className="voice-live-dot" aria-hidden />}
            {stateLine(floor, muted)}
          </p>
          <p className={`voice-heard${busy ? " is-older" : ""}`}>{tail(user)}</p>
        </div>
        <div className="voice-controls">
          <button
            className={`round-btn quiet${busy ? "" : " is-hidden"}`}
            aria-label="Stop the answer"
            tabIndex={busy ? 0 : -1}
            onClick={() => voiceSession().stopSpeaking()}
          >
            <StopIcon size={18} />
          </button>
          <button
            className={`round-btn quiet${muted ? " is-on" : ""}`}
            aria-pressed={muted}
            aria-label={muted ? "Unmute the microphone" : "Mute the microphone"}
            onClick={() => voiceSession().setMuted(!muted)}
          >
            {muted ? <MicOffIcon size={22} /> : <MicIcon size={22} />}
          </button>
          <button className="btn btn-primary btn-sm" onClick={() => void leaveVoice()}>
            End
          </button>
        </div>
      </div>
    </footer>
  );
}
