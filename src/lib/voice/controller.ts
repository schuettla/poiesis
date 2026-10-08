// Voice mode as the app uses it: one session, opened from a button, a hotkey or
// the command palette, closed whenever the user leaves (VXP-3). This file wires
// the real api, audio and chat store into `VoiceSession`; the logic is there.

import * as api from "../api";
import { useAppStore } from "../store";
import { VoiceAudio } from "./audio";
import { setSpeechBridge } from "./bridge";
import { VoiceSession, type VoiceApi } from "./session";
import { setVoiceUi, useVoiceStore } from "./voiceStore";

const realApi: VoiceApi = {
  start: (onEvent) => api.voiceStart(onEvent),
  stop: () => api.voiceStop(),
  push: (counter, pcm) => api.voicePushAudio(counter, pcm),
  assistant: (state, generation) => api.voiceAssistant(state, generation),
  speak: (generation, text, done, opts) => api.voiceSpeak(generation, text, done, opts),
  cancel: (generation) => api.voiceCancelSpeech(generation),
};

let session: VoiceSession | null = null;
let watching = false;

/** The one session. Made on first use and registered with the chat store. */
export function voiceSession(): VoiceSession {
  if (!session) {
    session = new VoiceSession({
      api: realApi,
      makeAudio: (events) => new VoiceAudio(events),
      host: {
        sendSpoken: (text) => useAppStore.getState().sendMessage(text, [], { spoken: true }),
        stopRun: () => useAppStore.getState().stopGenerating(),
        busy: () => useAppStore.getState().busy,
      },
    });
    setSpeechBridge(session);
  }
  watchForLeaving();
  return session;
}

/** The mic closes when the user moves on: another conversation, or the window
 * is minimized or hidden (VXP-3). */
function watchForLeaving(): void {
  if (watching || typeof document === "undefined") return;
  watching = true;
  useAppStore.subscribe((state, prev) => {
    if (state.activeConversationId !== prev.activeConversationId && session?.live) void leaveVoice();
  });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && session?.live) void leaveVoice();
  });
  window.addEventListener("beforeunload", () => {
    void session?.stop();
  });
}

export type OpenResult = "live" | "setup" | "error";

/** What still has to be downloaded before a voice conversation can work. Both
 * hearing and a voice are needed: one to understand, one to answer (VXP-7). */
export interface VoiceNeeds {
  /** Id of the hearing to get, or null when it is installed. */
  hearingId: string | null;
  hearingSize: string;
  /** Id of the voice model to get, or null when it is installed. */
  voiceModelId: string | null;
  voiceSize: string;
}

/** Null when everything is installed. */
export async function voiceNeeds(): Promise<VoiceNeeds | null> {
  const [status, settings, catalog] = await Promise.all([api.voiceStatus(), api.voiceSettings(), api.voiceCatalog()]);
  const voiceModel = catalog.voices.find((m) => m.voices.some((v) => v.id === settings.voice_id));
  const needs: VoiceNeeds = {
    hearingId: status.hearing ? null : settings.hearing_model,
    hearingSize: catalog.hearing.find((h) => h.id === settings.hearing_model)?.size_label ?? "",
    voiceModelId: !voiceModel || status.voices.includes(voiceModel.id) ? null : voiceModel.id,
    voiceSize: voiceModel?.size_label ?? "",
  };
  return needs.hearingId || needs.voiceModelId ? needs : null;
}

/** Shows the voice surface and starts listening. `setup` means something is not
 * installed yet: the surface offers the download and calls this again. */
export async function openVoice(): Promise<OpenResult> {
  // The voice surface lives in the chat view. Opening the mic anywhere it
  // cannot be seen would break VXP-3, so it does not open there.
  const app = useAppStore.getState();
  if (app.view !== "chat" || app.workspaceMode) return "error";
  setVoiceUi({ shown: true, error: null, used: true });
  const s = voiceSession();
  if (s.live) return "live";
  if (!api.inTauri()) {
    setVoiceUi({ error: "Voice works in the desktop app." });
    return "error";
  }
  if (!useAppStore.getState().activeConversationId) {
    setVoiceUi({ error: "Open a conversation first." });
    return "error";
  }
  const needs = await voiceNeeds().catch(() => undefined);
  if (needs === undefined) {
    setVoiceUi({ error: "Voice could not be checked. Try again." });
    return "error";
  }
  if (needs) return "setup";
  try {
    await s.start();
    return "live";
  } catch {
    return "error"; // the session put a plain sentence in the voice state
  }
}

/** Closes the mic and the surface. */
export async function leaveVoice(): Promise<void> {
  setVoiceUi({ shown: false });
  await session?.stop();
}

/** "Show chat": the surface goes, the session and its mic stay open, and the
 * mic indicator in the top bar keeps saying so (VXP-3). */
export function hideVoiceSurface(): void {
  setVoiceUi({ shown: false });
}

/** True while the mic is open anywhere in the app. */
export const useMicOpen = (): boolean => useVoiceStore((s) => s.floor !== "off" && s.floor !== "starting" && !s.muted);
