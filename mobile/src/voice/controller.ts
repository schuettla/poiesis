// Voice on the phone: one session, opened from the mic button and closed whenever the person leaves. This file
// wires the link, the audio and the store into `VoiceSession`; the logic is there and is tested with fakes.

import { useStore } from "../store";
import { makeVoiceApi, voiceSetupLine } from "./api";
import { VoiceAudio } from "./audio";
import { setSpeechBridge } from "./bridge";
import { VoiceSession } from "./session";
import { setVoiceUi } from "./voiceStore";

let session: VoiceSession | null = null;
let watching = false;

/** The one session. Made on first use and registered as the store's speech bridge. */
export function voiceSession(): VoiceSession {
  if (!session) {
    session = new VoiceSession({
      // The stream ended by itself: the link dropped or the desktop closed the session.
      api: makeVoiceApi((why) => {
        if (!session?.live) return;
        void session.stop();
        setVoiceUi({ shown: false });
        useStore.getState().tell(why ? `Voice stopped. ${why}` : "Voice stopped.");
      }),
      makeAudio: (events) => new VoiceAudio(events),
      host: {
        sendSpoken: (text) => useStore.getState().send(text, [], { spoken: true }),
        stopRun: () => void useStore.getState().stop(),
        busy: () => Object.values(useStore.getState().live).some((l) => l.running),
      },
    });
    setSpeechBridge(session);
  }
  watchForLeaving();
  return session;
}

/** Voice ends when the app goes to the background or the screen turns off: there is no background audio (plan, II.3). */
function watchForLeaving(): void {
  if (watching || typeof document === "undefined") return;
  watching = true;
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && session?.live) {
      void leaveVoice();
      useStore.getState().tell("Voice stopped when you left the app.");
    }
  });
}

export type OpenResult = "live" | "error";

/** Shows the voice screen and starts listening. A plain sentence is left in the voice state when it cannot. */
export async function openVoice(): Promise<OpenResult> {
  setVoiceUi({ shown: true, error: null, used: true });
  const s = voiceSession();
  if (s.live) return "live";
  try {
    const setup = await voiceSetupLine();
    if (setup) {
      setVoiceUi({ error: setup });
      return "error";
    }
    await s.start();
    return "live";
  } catch (e) {
    // The session put its own sentence in the voice state; a failure before it began gets one here.
    setVoiceUi({ error: typeof e === "string" && e ? e : "Voice could not be started." });
    return "error";
  }
}

/** Closes the microphone and the screen. */
export async function leaveVoice(): Promise<void> {
  setVoiceUi({ shown: false });
  await session?.stop();
}
