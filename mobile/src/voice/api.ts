/**
 * The voice session's way to the desktop (REM-19). The desktop hears, thinks and speaks; the phone only carries
 * the microphone up and the speech down. One long stream holds the session: events come down it, microphone
 * batches go up it as binary frames, and closing it ends the session on the desktop.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import * as link from "../link";
import type { VoiceEvent } from "./events";
import type { VoiceApi } from "./session";

/** The desktop says `ready` once the session is open. */
function isReady(e: unknown): boolean {
  return !!e && typeof e === "object" && (e as { type?: unknown }).type === "ready";
}

/** One microphone batch as the desktop reads it: an 8 byte little-endian counter, then 16 kHz 16-bit mono. */
export function pushBody(counter: number, pcm: Uint8Array): Uint8Array {
  const body = new Uint8Array(8 + pcm.length);
  new DataView(body.buffer).setBigUint64(0, BigInt(counter), true);
  body.set(pcm, 8);
  return body;
}

/** `onEnded` is told when the stream ends by itself (the link dropped, the desktop closed the session). */
export function makeVoiceApi(onEnded: (why: string | null) => void): VoiceApi {
  return {
    start: (onEvent) =>
      new Promise<void>((resolve, reject) => {
        let ready = false;
        const channel = new Channel<unknown>();
        channel.onmessage = (e) => {
          if (isReady(e)) {
            ready = true;
            resolve();
          } else onEvent(e as VoiceEvent);
        };
        invoke<void>("remote_voice_open_cmd", { args: { ui_language: navigator.language }, onEvent: channel }).then(
          () => (ready ? onEnded(null) : reject("Voice stopped before it began.")),
          (e) => (ready ? onEnded(link.words(e)) : reject(link.words(e))),
        );
      }),
    stop: async () => {
      await invoke<void>("remote_voice_close_cmd");
    },
    // Not a raw body: on Android every call goes through postMessage, which only carries JSON. Tauri turns the
    // bytes into a list of numbers, and the shell reads them back as bytes.
    push: (counter, pcm) => invoke<void>("remote_voice_push_cmd", { audio: pushBody(counter, pcm) }),
    assistant: async (state, generation) => {
      await link.call("voice_assistant", { state, generation });
    },
    speak: async (generation, text, done, opts) => {
      await link.call("voice_speak", { generation, text, done, notice: opts?.notice, language: opts?.language });
    },
    cancel: async (generation) => {
      await link.call("voice_cancel_speech", { generation });
    },
  };
}

/** What the desktop has installed for voice. */
export interface VoiceStatus {
  hearing: string | null;
  voices: string[];
}

/** One sentence when voice cannot start because the desktop has no hearing or no voice yet, else null. */
export async function voiceSetupLine(): Promise<string | null> {
  const status = await link.call<VoiceStatus>("voice_status");
  if (!status.hearing || status.voices.length === 0) {
    return "Voice is not set up on your desktop. Open Settings and then Voice there.";
  }
  return null;
}
