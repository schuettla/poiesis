import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceEvent } from "../api";
import type { VoiceAudioEvents } from "./audio";
import { decodePcm16, VoiceSession, type AudioPort, type VoiceApi } from "./session";
import { initialVoiceUi, resetVoiceUi, useVoiceStore } from "./voiceStore";

function pcmBase64(samples: number[]): string {
  const bytes = new Uint8Array(samples.length * 2);
  samples.forEach((s, i) => new DataView(bytes.buffer).setInt16(i * 2, s, true));
  return btoa(String.fromCharCode(...bytes));
}

function rig(opts: { busy?: () => boolean } = {}) {
  let emit: (e: VoiceEvent) => void = () => undefined;
  let audioEvents: VoiceAudioEvents = {};
  let clock = 100_000;
  const api = {
    start: vi.fn(async (onEvent: (e: VoiceEvent) => void) => {
      emit = onEvent;
    }),
    stop: vi.fn(async () => undefined),
    push: vi.fn(async () => undefined),
    assistant: vi.fn(async (_state: "started" | "finished", _generation: number) => undefined),
    speak: vi.fn(async () => undefined),
    cancel: vi.fn(async () => undefined),
  } satisfies VoiceApi;
  const audio = {
    start: vi.fn(async () => undefined),
    stop: vi.fn(),
    enqueue: vi.fn(),
    cancel: vi.fn(),
    setDuck: vi.fn(),
    setMuted: vi.fn(),
  } satisfies AudioPort;
  const sent: string[] = [];
  // Like the real store, a spoken turn stays open until the test ends it.
  let finishTurn: () => void = () => undefined;
  const host = {
    sendSpoken: vi.fn(
      (text: string) =>
        new Promise<void>((resolve) => {
          sent.push(text);
          finishTurn = resolve;
        })
    ),
    stopRun: vi.fn(),
    busy: opts.busy ?? (() => false),
  };
  const session = new VoiceSession({
    api,
    makeAudio: (events) => {
      audioEvents = events;
      return audio;
    },
    host,
    now: () => clock,
    busyWaitMs: 100,
  });
  return {
    session,
    api,
    audio,
    host,
    sent,
    endTurn: () => finishTurn(),
    emit: (e: VoiceEvent) => emit(e),
    audioEvents: () => audioEvents,
    tick: (ms: number) => {
      clock += ms;
    },
  };
}

const audioEvent = (generation: number, seq: number, text: string, notice = false): VoiceEvent => ({
  type: "audio",
  generation,
  seq,
  sample_rate: 24000,
  pcm: pcmBase64([0, 16384, -16384]),
  text,
  notice,
});

/** Starts the session and delivers one finished user turn; returns its reply id. */
async function startAndHear(r: ReturnType<typeof rig>, words = "What is the weather?"): Promise<number> {
  await r.session.start();
  r.emit({ type: "transcript", text: words, language: "en" });
  await vi.waitFor(() => expect(r.host.sendSpoken).toHaveBeenCalled());
  return r.session.current();
}

beforeEach(() => {
  resetVoiceUi();
});

describe("decodePcm16", () => {
  it("turns little-endian 16-bit samples into floats", () => {
    const out = decodePcm16(pcmBase64([0, 16384, -16384, 32767, -32768]));
    expect([...out]).toEqual([0, 0.5, -0.5, 32767 / 32768, -1]);
  });
});

describe("VoiceSession", () => {
  it("opens the session and the mic, and closes both on stop (VXP-3)", async () => {
    const r = rig();
    await r.session.start();
    expect(r.audio.start).toHaveBeenCalledWith({ playback: true });
    expect(useVoiceStore.getState().floor).toBe("listening");
    await r.session.stop();
    expect(r.audio.stop).toHaveBeenCalled();
    expect(r.api.stop).toHaveBeenCalled();
    expect(useVoiceStore.getState().floor).toBe("off");
  });

  it("shows a plain sentence and closes everything when the session cannot start", async () => {
    const r = rig();
    r.api.start.mockRejectedValueOnce("Poiesis needs to download its hearing before it can listen.");
    await expect(r.session.start()).rejects.toBeTruthy();
    expect(useVoiceStore.getState().error).toBe("Poiesis needs to download its hearing before it can listen.");
    expect(useVoiceStore.getState().floor).toBe("off");
    expect(r.audio.start).not.toHaveBeenCalled();
  });

  it("closes the session when the mic is refused", async () => {
    const r = rig();
    r.audio.start.mockRejectedValueOnce(new Error("No microphone was found. Plug one in and try again."));
    await expect(r.session.start()).rejects.toBeTruthy();
    expect(useVoiceStore.getState().error).toContain("No microphone");
    expect(r.api.stop).toHaveBeenCalled();
    expect(r.session.live).toBe(false);
  });

  it("sends mic frames five at a time with a rising counter (AUD-2)", async () => {
    const r = rig();
    await r.session.start();
    const frame = new Int16Array(320).fill(3);
    for (let i = 0; i < 11; i++) r.audioEvents().onFrame?.(frame, i);
    expect(r.api.push).toHaveBeenCalledTimes(2);
    const calls = r.api.push.mock.calls as unknown as [number, Uint8Array][];
    expect(calls.map((c) => c[0])).toEqual([1, 2]);
    expect(calls[0][1].length).toBe(5 * 320 * 2);
  });

  it("sends no sound while muted", async () => {
    const r = rig();
    await r.session.start();
    r.session.setMuted(true);
    for (let i = 0; i < 10; i++) r.audioEvents().onFrame?.(new Int16Array(320), i);
    expect(r.api.push).not.toHaveBeenCalled();
    expect(r.audio.setMuted).toHaveBeenCalledWith(true);
  });

  it("sends the finished turn as a spoken message", async () => {
    const r = rig();
    await startAndHear(r, "Hello there");
    expect(r.sent).toEqual(["Hello there"]);
    expect(useVoiceStore.getState().user).toBe("Hello there");
  });

  it("streams the reply to be spoken and ends it when asked", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.session.token(g, "It is sunny. ");
    expect(r.api.speak).toHaveBeenCalledWith(g, "It is sunny. ", false);
    const ended = r.session.end(g);
    expect(r.api.speak).toHaveBeenLastCalledWith(g, "", true);

    r.emit(audioEvent(g, 0, "It is sunny."));
    expect(r.audio.enqueue).toHaveBeenCalledTimes(1);
    expect(r.api.assistant).toHaveBeenCalledWith("started", g);
    r.emit({ type: "speech_done", generation: g });
    // Still being played: the turn is not over.
    let settled = false;
    void ended.then(() => (settled = true));
    await Promise.resolve();
    expect(settled).toBe(false);

    r.audioEvents().onPlayed?.(g, 0);
    await expect(ended).resolves.toEqual({ interrupted: false, heard: "It is sunny." });
    expect(r.api.assistant).toHaveBeenCalledWith("finished", g);
  });

  it("tells Rust Poiesis started speaking only once per reply", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.emit(audioEvent(g, 0, "One."));
    r.emit(audioEvent(g, 1, "Two."));
    expect(r.api.assistant.mock.calls.filter((c) => c[0] === "started")).toHaveLength(1);
  });

  it("ducks and restores the voice (TRN-2)", async () => {
    const r = rig();
    await r.session.start();
    r.emit({ type: "duck" });
    expect(r.audio.setDuck).toHaveBeenLastCalledWith(0.18, 0.02);
    r.emit({ type: "unduck" });
    expect(r.audio.setDuck).toHaveBeenLastCalledWith(1, 0.12);
  });

  it("a cut-in stops the voice, the run and what is saved is only what was heard (VTN-6)", async () => {
    const r = rig();
    const g = await startAndHear(r);
    const ended = r.session.end(g);
    r.emit(audioEvent(g, 0, "First sentence."));
    r.emit(audioEvent(g, 1, "Second sentence."));
    r.emit(audioEvent(g, 2, "Third sentence."));
    r.audioEvents().onPlayed?.(g, 0);
    r.emit({ type: "yield" });

    expect(r.audio.cancel).toHaveBeenCalledWith(g);
    expect(r.api.cancel).toHaveBeenCalledWith(g);
    expect(r.host.stopRun).toHaveBeenCalledTimes(1);
    await expect(ended).resolves.toEqual({ interrupted: true, heard: "First sentence." });

    // Pieces that were already on their way are dropped, and so is late text.
    r.emit(audioEvent(g, 3, "Too late."));
    expect(r.audio.enqueue).toHaveBeenCalledTimes(3);
    r.session.token(g, "more text");
    expect(r.api.speak).not.toHaveBeenCalledWith(g, "more text", false);
  });

  it("Stop silences a reply even when the run has already ended", async () => {
    const r = rig();
    const g = await startAndHear(r);
    const ended = r.session.end(g);
    r.emit(audioEvent(g, 0, "Hello."));
    r.session.stopSpeaking();
    await expect(ended).resolves.toMatchObject({ interrupted: true, heard: "" });
    // Stopping twice, or stopping something already over, does nothing more.
    r.session.stopSpeaking();
    expect(r.host.stopRun).toHaveBeenCalledTimes(1);
  });

  it("leaving voice mode mid-reply counts as cutting it off", async () => {
    const r = rig();
    const g = await startAndHear(r);
    const ended = r.session.end(g);
    r.emit(audioEvent(g, 0, "Hello."));
    await r.session.stop();
    await expect(ended).resolves.toMatchObject({ interrupted: true });
  });

  it("a reply with nothing to say still finishes", async () => {
    const r = rig();
    const g = await startAndHear(r);
    const ended = r.session.end(g);
    r.emit({ type: "speech_done", generation: g });
    await expect(ended).resolves.toEqual({ interrupted: false, heard: "" });
  });

  it("ignores audio and text for a reply that is no longer current", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.session.token(g - 1, "old");
    r.emit(audioEvent(g - 1, 0, "old"));
    expect(r.api.speak).not.toHaveBeenCalled();
    expect(r.audio.enqueue).not.toHaveBeenCalled();
  });

  it("speaks one short notice while a tool runs, and not again for ten seconds (VTN-5)", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.tick(2000);
    r.session.step(g, "searched the web");
    expect(r.api.speak).toHaveBeenCalledWith(g, "Let me look that up.", false, { notice: true, language: "en" });
    r.tick(3000);
    r.session.step(g, "ran a script");
    expect(r.api.speak).toHaveBeenCalledTimes(1);
    r.tick(8000);
    r.session.step(g, "ran a script");
    expect(r.api.speak).toHaveBeenCalledTimes(2);
  });

  it("does not speak a notice while something is still being said", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.emit(audioEvent(g, 0, "Let me think about that."));
    r.tick(5000);
    r.session.step(g, "searched the web");
    expect(r.api.speak).not.toHaveBeenCalled();
  });

  it("speaks a notice in German to a German speaker", async () => {
    const r = rig();
    await r.session.start();
    r.emit({ type: "transcript", text: "Wie ist das Wetter?", language: "de" });
    await vi.waitFor(() => expect(r.host.sendSpoken).toHaveBeenCalled());
    r.tick(2000);
    r.session.step(r.session.current(), "searched the web");
    expect(r.api.speak).toHaveBeenCalledWith(r.session.current(), "Ich schau kurz nach.", false, {
      notice: true,
      language: "de",
    });
  });

  it("gives the floor back when the turn ends without any speech", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.endTurn();
    await vi.waitFor(() => expect(r.api.assistant).toHaveBeenCalledWith("finished", g));
  });

  it("waits briefly for a busy run before sending the next turn", async () => {
    let busy = true;
    const r = rig({ busy: () => busy });
    await r.session.start();
    r.emit({ type: "transcript", text: "Next question", language: "en" });
    await new Promise((res) => setTimeout(res, 20));
    expect(r.host.sendSpoken).not.toHaveBeenCalled();
    busy = false;
    await vi.waitFor(() => expect(r.host.sendSpoken).toHaveBeenCalledWith("Next question"));
  });

  it("shows the sentence being said and the hint from Rust", async () => {
    const r = rig();
    const g = await startAndHear(r);
    r.emit(audioEvent(g, 0, "First."));
    r.emit(audioEvent(g, 1, "Second."));
    expect(useVoiceStore.getState().reply).toBe("First.");
    r.audioEvents().onPlayed?.(g, 0);
    expect(useVoiceStore.getState().reply).toBe("Second.");
    r.emit({ type: "hint", text: "This reply is in German, but no German voice is installed." });
    expect(useVoiceStore.getState().hint).toContain("German");
  });
});

describe("voice ui state", () => {
  it("starts off", () => {
    expect(useVoiceStore.getState()).toMatchObject({ floor: initialVoiceUi.floor, shown: false });
  });
});
