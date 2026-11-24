/**
 * `link.ts` against a fake shell: the names of the commands it calls, the arguments it sends, and events
 * arriving on a channel. The shell holds the key; the page must never be handed one.
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

import { call, connect, forget, pair, state, stream, words } from "./link";

beforeEach(() => {
  shell.calls.length = 0;
  shell.answer(async () => undefined);
});

describe("the link", () => {
  it("calls the shell's commands by name", async () => {
    await state();
    await connect();
    await pair("{}", "Pixel");
    await forget();
    expect(shell.calls.map((c) => c[0])).toEqual(["remote_state_cmd", "remote_connect_cmd", "remote_pair_cmd", "remote_forget_cmd"]);
    expect(shell.calls[2][1]).toEqual({ text: "{}", label: "Pixel" });
  });

  it("sends a command and its arguments as one call", async () => {
    shell.answer(async () => [{ id: "c1" }]);
    expect(await call("messages", { conversation_id: "c1" })).toEqual([{ id: "c1" }]);
    expect(shell.calls[0]).toEqual(["remote_call_cmd", { cmd: "messages", args: { conversation_id: "c1" } }]);
    await call("conversations");
    expect(shell.calls[1]).toEqual(["remote_call_cmd", { cmd: "conversations", args: {} }]);
  });

  it("hands events to the page as they come and resolves when the command is done", async () => {
    const seen: unknown[] = [];
    shell.answer(async (_cmd, args) => {
      const { onEvent } = args as { onEvent: { onmessage: (m: unknown) => void } };
      onEvent.onmessage({ type: "token", text: "a" });
      onEvent.onmessage({ type: "token", text: "b" });
    });
    await stream("send", { text: "hi" }, (e) => seen.push(e));
    expect(seen).toEqual([
      { type: "token", text: "a" },
      { type: "token", text: "b" },
    ]);
    expect(shell.calls[0][0]).toBe("remote_stream_cmd");
    expect(shell.calls[0][1]).toMatchObject({ cmd: "send", args: { text: "hi" } });
  });

  it("passes a failure on as the shell's sentence", async () => {
    shell.answer(() => Promise.reject("This phone is not paired yet."));
    await expect(call("status")).rejects.toBe("This phone is not paired yet.");
  });

  it("finds words for any rejection", () => {
    expect(words("plain")).toBe("plain");
    expect(words(new Error("boom"))).toBe("boom");
    expect(words(42)).toBe("Something went wrong.");
  });
});
