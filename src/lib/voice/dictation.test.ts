import { describe, expect, it, vi } from "vitest";
import { Dictation, MAX_DICTATION_SECONDS, joinSpoken, type MicFactory } from "./dictation";

function loud(): Int16Array {
  return new Int16Array(320).fill(4000);
}

/** A mic that lets the test push frames by hand. */
function fakeMic() {
  const state = { push: (_pcm: Int16Array) => {}, started: false, stopped: 0, failWith: null as Error | null };
  const factory: MicFactory = (events) => {
    state.push = events.onFrame;
    return {
      start: async () => {
        if (state.failWith) throw state.failWith;
        state.started = true;
      },
      stop: () => {
        state.stopped++;
      },
    };
  };
  return { state, factory };
}

describe("Dictation", () => {
  it("sends what was recorded and returns the text", async () => {
    const { state, factory } = fakeMic();
    const transcribe = vi.fn(async (_pcm: Uint8Array) => "  hello there ");
    const d = new Dictation(factory, transcribe);
    await d.start();
    for (let i = 0; i < 60; i++) state.push(loud());
    expect(await d.finish()).toBe("hello there");
    const bytes = transcribe.mock.calls[0][0];
    expect(bytes.length).toBe(60 * 320 * 2);
    expect(state.stopped).toBe(1);
    expect(d.listening).toBe(false);
  });

  it("treats a tap or pure silence as nothing said, without calling the engine", async () => {
    const { state, factory } = fakeMic();
    const transcribe = vi.fn(async (_pcm: Uint8Array) => "x");
    const d = new Dictation(factory, transcribe);
    await d.start();
    for (let i = 0; i < 5; i++) state.push(loud());
    expect(await d.finish()).toBe("");
    await d.start();
    for (let i = 0; i < 60; i++) state.push(new Int16Array(320));
    expect(await d.finish()).toBe("");
    expect(transcribe).not.toHaveBeenCalled();
  });

  it("closes the mic on cancel and keeps nothing", async () => {
    const { state, factory } = fakeMic();
    const transcribe = vi.fn(async (_pcm: Uint8Array) => "x");
    const d = new Dictation(factory, transcribe);
    await d.start();
    state.push(loud());
    d.cancel();
    expect(state.stopped).toBe(1);
    expect(d.listening).toBe(false);
  });

  it("is not left listening when the mic fails to open", async () => {
    const { state, factory } = fakeMic();
    state.failWith = new Error("denied");
    const d = new Dictation(factory, async () => "x");
    await expect(d.start()).rejects.toThrow("denied");
    expect(d.listening).toBe(false);
  });

  it("says so once when the time limit is reached and ignores later frames", async () => {
    const { state, factory } = fakeMic();
    const onLimit = vi.fn();
    const transcribe = vi.fn(async (_pcm: Uint8Array) => "long");
    const d = new Dictation(factory, transcribe, { onLimit });
    await d.start();
    const max = MAX_DICTATION_SECONDS * 50;
    for (let i = 0; i < max + 20; i++) state.push(loud());
    expect(onLimit).toHaveBeenCalledTimes(1);
    await d.finish();
    const bytes = transcribe.mock.calls[0][0];
    expect(bytes.length).toBe(max * 320 * 2);
  });
});

describe("joinSpoken", () => {
  it("adds spoken words after typed text with one space", () => {
    expect(joinSpoken("write a note ", "about the trip")).toBe("write a note about the trip");
  });
  it("uses the spoken words alone when nothing is typed", () => {
    expect(joinSpoken("", "hello")).toBe("hello");
    expect(joinSpoken("  ", "hello")).toBe("hello");
  });
});
