import { useState } from "react";
import { useAppStore } from "../../lib/store";
import { openVoice } from "../../lib/voice/controller";
import { useVoiceStore } from "../../lib/voice/voiceStore";
import { ChevronIcon, MicIcon, WaveformIcon } from "../Icons/Icons";
import MicButton from "./MicButton";
import VoiceButton from "./VoiceButton";

/**
 * Voice, under the composer on the left: the mic dictates into the box as it
 * always has, and the chevron beside it opens a drop-up with the switch into
 * voice mode — the back-and-forth conversation that `/talk` also starts. While
 * a voice conversation is live but hidden, `VoiceButton` stays one click away.
 */
export default function VoiceControl(props: React.ComponentProps<typeof MicButton>) {
  const [open, setOpen] = useState(false);
  const workspaceMode = useAppStore((s) => s.workspaceMode);
  const live = useVoiceStore((s) => s.floor !== "off");

  return (
    <div className="voice-control">
      <MicButton {...props} />
      <div className="composer-menu-wrap">
        <button
          className={`icon-btn voice-more ${open ? "on" : ""}`}
          aria-label="Voice options"
          aria-haspopup="menu"
          aria-expanded={open}
          title="Voice options"
          onClick={() => setOpen((v) => !v)}
        >
          <ChevronIcon dir={open ? "down" : "up"} size={10} strokeWidth={1.8} />
        </button>
        {open && (
          <>
            <div className="composer-menu-backdrop" onClick={() => setOpen(false)} />
            <div className="composer-menu voice-menu" role="menu">
              <button
                className="composer-menu-item"
                role="menuitem"
                disabled={workspaceMode}
                onClick={() => {
                  setOpen(false);
                  void openVoice();
                }}
              >
                <span className="mi-icon" aria-hidden="true"><WaveformIcon size={15} /></span>
                <span className="mi-body">
                  {live ? "Back to the voice conversation" : "Voice mode"}
                  <span className="mi-hint">
                    {workspaceMode
                      ? "opens in the chat view, leave workspace mode first"
                      : live
                        ? "the microphone is still on"
                        : "talk back and forth, I answer out loud"}
                  </span>
                </span>
                <span className="mi-check" />
              </button>
              <div className="composer-menu-sep" />
              <div className="voice-menu-note">
                <span className="mi-icon" aria-hidden="true"><MicIcon size={13} /></span>
                Tap the mic to dictate, hold it to talk and send.
              </div>
            </div>
          </>
        )}
      </div>
      <VoiceButton />
    </div>
  );
}
