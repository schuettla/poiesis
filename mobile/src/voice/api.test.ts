/**
 * The voice link (REM-19): the layout of one microphone batch, and the start that waits for the desktop to say
 * `ready` before the session counts as open.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

type Args = unknown[];

const shell = vi.hoisted(() => {
  const calls: Args[] = [];
  let reply: (...a: Args) => Promise<unknown> = async () => undefined;
  return {
    calls,
    answer: (f: (...a: Args) => Promise<unknown>) => {
      reply = f;
    },
    invoke: (...a: Args) => {
      calls.push(a);
      return reply(...a);
    },
    Channel: class FakeChannel<T> {
      onmessage: (m: T) => void = () => {};
    },
  };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: shell.invoke, Channel: shell.Channel }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

import { makeVoiceApi, pushBody, voiceSetupLine } from "./api";

beforeEach(() => {
  shell.calls.length = 0;
  shell.answer(async () => undefined);
});

describe("one microphone batch", () => {
  it("is an 8 byte little-endian counter and then the audio as it came", () => {
    const body = pushBody(258, new Uint8Array([1, 2, 3, 4]));
    expect(Array.from(body.slice(0, 8))).toEqual([2, 1, 0, 0, 0, 0, 0, 0]);
    expect(Array.from(body.slice(8))).toEqual([1, 2, 3, 4]);
    expect(body.length).toBe(12);
  });
});

describe("starting voice", () => {
  it("is open when the desktop says ready, and passes later events on", async () => {
    const seen: unknown[] = [];
    let ended: string | null | undefined;
    shell.answer(async (_cmd, args) => {
      const { onEvent } = args as { onEvent: { onmessage: (m: unknown) => void } };
      onEvent.onmessage({ type: "ready" });
      onEvent.onmessage({ type: "floor", state: "listening" });
    });
    const api = makeVoiceApi((why) => (ended = why));
    await api.start((e) => seen.push(e));
    expect(seen).toEqual([{ type: "floor", state: "listening" }]);
    expect(shell.calls[0][0]).toBe("remote_voice_open_cmd");
    await Promise.resolve();
    expect(ended).toBeNull(); // the stream ended after it was open, and the session is told
  });

  it("fails with the desktop's sentence when it could not open", async () => {
    shell.answer(() => Promise.reject("Poiesis needs to download its hearing before it can listen."));
    const api = makeVoiceApi(() => {});
    await expect(api.start(() => {})).rejects.toBe("Poiesis needs to download its hearing before it can listen.");
  });

  it("tells the session when the link breaks while it is open", async () => {
    let ended: string | null | undefined;
    shell.answer(async (_cmd, args) => {
      (args as { onEvent: { onmessage: (m: unknown) => void } }).onEvent.onmessage({ type: "ready" });
      throw "I can't reach your desktop.";
    });
    const api = makeVoiceApi((why) => (ended = why));
    await api.start(() => {});
    await Promise.resolve();
    await Promise.resolve();
    expect(ended).toBe("I can't reach your desktop.");
  });

  it("sends the controls as calls the desktop knows", async () => {
    const api = makeVoiceApi(() => {});
    await api.assistant("started", 3);
    await api.speak(3, "Hello", false, { notice: true, language: "de" });
    await api.cancel(3);
    await api.push(1, new Uint8Array([0, 0]));
    await api.stop();
    expect(shell.calls.map((c) => (c[1] as { cmd?: string })?.cmd ?? c[0])).toEqual([
      "voice_assistant",
      "voice_speak",
      "voice_cancel_speech",
      "remote_voice_push_cmd",
      "remote_voice_close_cmd",
    ]);
    expect(shell.calls[1][1]).toEqual({ cmd: "voice_speak", args: { generation: 3, text: "Hello", done: false, notice: true, language: "de" } });
    // Inside an object, never a raw body: Android carries only JSON, and a raw body would arrive as something else.
    expect(shell.calls[3][1]).toEqual({ audio: pushBody(1, new Uint8Array([0, 0])) });
  });
});

describe("whether the desktop can do voice", () => {
  it("says one plain line when hearing or a voice is missing", async () => {
    shell.answer(async () => ({ hearing: null, voices: ["v"] }));
    expect(await voiceSetupLine()).toContain("not set up on your desktop");
    shell.answer(async () => ({ hearing: "h", voices: [] }));
    expect(await voiceSetupLine()).toContain("not set up on your desktop");
    shell.answer(async () => ({ hearing: "h", voices: ["v"] }));
    expect(await voiceSetupLine()).toBeNull();
  });
});
