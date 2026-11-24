/**
 * The phone's store against a fake link: pairing, a turn that streams and is then replaced by what the desktop
 * kept, questions that open and close from both sides, and the order of the chat list.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const state = vi.fn();
const connect = vi.fn();
const pair = vi.fn();
const forget = vi.fn();
const call = vi.fn();
const stream = vi.fn();
vi.mock("./link", () => ({
  state: (...a: unknown[]) => state(...a),
  connect: (...a: unknown[]) => connect(...a),
  pair: (...a: unknown[]) => pair(...a),
  forget: (...a: unknown[]) => forget(...a),
  call: (...a: unknown[]) => call(...a),
  stream: (...a: unknown[]) => stream(...a),
  onLink: async () => () => {},
  words: (e: unknown) => (typeof e === "string" ? e : "Something went wrong."),
}));

import { setSpeechBridge, type SpeechBridge } from "./voice/bridge";
import { onFeed, onTurnEvent, orderChats, startFeed, stopFeed, useStore } from "./store";
import type { ChatRow } from "./types";

const row = (id: string, updated_at: number, remote_sender: string | null = null): ChatRow => ({
  id,
  title: id,
  updated_at,
  remote_sender,
  remote_transport: remote_sender ? "phone" : null,
});

const fresh = () =>
  useStore.setState({
    ready: false,
    paired: false,
    desktop: null,
    link: "connecting",
    screen: "chats",
    chats: [],
    active: null,
    messages: {},
    artifacts: {},
    live: {},
    approvals: [],
    waiting: [],
    error: null,
  });

/** Answers for the calls a screen makes. */
function desktop(over: Record<string, unknown> = {}) {
  const answers: Record<string, unknown> = {
    conversations: [row("c1", 10)],
    waiting: [],
    messages: [],
    artifacts: [],
    open_chat: { conversation_id: "c1" },
    new_chat: { conversation_id: "new1" },
    answer: { answered: true },
    stop: { stopped: true },
    ...over,
  };
  call.mockImplementation(async (cmd: string) => {
    const a = answers[cmd];
    if (a instanceof Error) throw a.message;
    return a;
  });
}

beforeEach(() => {
  for (const m of [state, connect, pair, forget, call, stream]) m.mockReset();
  stopFeed();
  fresh();
  connect.mockResolvedValue({ paired: true, name: "Desk", link: "connected" });
  stream.mockResolvedValue(undefined);
  desktop();
});

describe("starting", () => {
  it("asks to be paired when it has no desktop", async () => {
    state.mockResolvedValue({ paired: false, name: null, link: "connecting" });
    await useStore.getState().boot();
    expect(useStore.getState().paired).toBe(false);
    expect(useStore.getState().ready).toBe(true);
    expect(call).not.toHaveBeenCalled();
  });

  it("loads the chats and the waiting list when it is paired", async () => {
    state.mockResolvedValue({ paired: true, name: "Desk", link: "connected" });
    desktop({ conversations: [row("c1", 10), row("c2", 20)], waiting: [{ id: "w1", summary: "Accept?" }] });
    await useStore.getState().boot();
    const s = useStore.getState();
    expect(s.desktop).toBe("Desk");
    expect(s.chats.map((c) => c.id)).toEqual(["c2", "c1"]);
    expect(s.waiting).toHaveLength(1);
    expect(s.link).toBe("connected");
  });

  it("pairs with the text and the name, and says the desktop's words when it fails", async () => {
    pair.mockResolvedValue("Erich's desktop");
    expect(await useStore.getState().pairWith("{}", "Pixel")).toBe(true);
    expect(pair).toHaveBeenCalledWith("{}", "Pixel");
    expect(useStore.getState().desktop).toBe("Erich's desktop");

    fresh();
    pair.mockRejectedValue("That code has run out. Ask the desktop for a new one.");
    expect(await useStore.getState().pairWith("{}", "Pixel")).toBe(false);
    expect(useStore.getState().paired).toBe(false);
    expect(useStore.getState().error).toBe("That code has run out. Ask the desktop for a new one.");
    stopFeed();
  });

  it("forgets the desktop and empties everything it had fetched", async () => {
    useStore.setState({ paired: true, desktop: "Desk", chats: [row("c1", 1)], approvals: [{ id: "a", summary: "s", path: "", conversationId: "c1" }] });
    await useStore.getState().unpair();
    const s = useStore.getState();
    expect(forget).toHaveBeenCalled();
    expect([s.paired, s.chats.length, s.approvals.length]).toEqual([false, 0, 0]);
  });
});

describe("the chat list", () => {
  it("puts the chats started from a phone first, then the newest", () => {
    const order = orderChats([row("a", 30), row("b", 10, "Pixel"), row("c", 20), row("d", 40, "Pixel")]);
    expect(order.map((c) => c.id)).toEqual(["d", "b", "a", "c"]);
  });
});

describe("a message", () => {
  it("streams the answer, then shows what the desktop kept in its place", async () => {
    useStore.setState({ paired: true, active: "c1", screen: "chat" });
    desktop({
      messages: [
        { id: "u1", role: "user", content: "hi", created_at: 1 },
        { id: "a1", role: "assistant", content: "Hello there", created_at: 2 },
      ],
    });
    let during = "";
    stream.mockImplementation(async (_cmd: string, _args: unknown, on: (e: unknown) => void) => {
      on({ type: "chat", conversation_id: "c1" });
      on({ type: "token", text: "Hello" });
      on({ type: "token", text: " there" });
      during = useStore.getState().live.c1?.text ?? "";
    });
    await useStore.getState().send("hi");
    expect(stream).toHaveBeenCalledWith("send", { text: "hi", conversation_id: "c1", attachments: [], spoken: false }, expect.any(Function));
    expect(during).toBe("Hello there");
    const s = useStore.getState();
    expect(s.messages.c1.map((m) => m.id)).toEqual(["u1", "a1"]);
    expect(s.live.c1).toBeUndefined();
  });

  it("starts a chat first when there is none open", async () => {
    useStore.setState({ paired: true, active: null });
    await useStore.getState().send("hello");
    expect(call).toHaveBeenCalledWith("new_chat");
    expect(stream).toHaveBeenCalledWith("send", expect.objectContaining({ conversation_id: "new1" }), expect.any(Function));
  });

  it("sends photos as data and nothing else", async () => {
    useStore.setState({ paired: true, active: "c1" });
    await useStore.getState().send("look", [{ name: "a.jpg", dataUri: "data:image/jpeg;base64,AA" }]);
    expect(stream.mock.calls[0][1]).toMatchObject({
      attachments: [{ kind: "image", name: "a.jpg", dataUri: "data:image/jpeg;base64,AA" }],
    });
  });

  it("keeps the desktop's sentence on screen when the turn fails, and does not send an empty message", async () => {
    useStore.setState({ paired: true, active: "c1" });
    stream.mockRejectedValue("I can't reach your desktop. It may be asleep, or Poiesis may not be running.");
    await useStore.getState().send("hi");
    expect(useStore.getState().live.c1.error).toContain("I can't reach your desktop");
    expect(useStore.getState().live.c1.running).toBe(false);

    stream.mockClear();
    await useStore.getState().send("   ");
    expect(stream).not.toHaveBeenCalled();
  });

  it("says a message added to a running turn was added", () => {
    useStore.setState({ live: { c1: { text: "", notice: null, error: null, artifacts: [], notes: [], helper: false, running: true } } });
    onTurnEvent("c1", { type: "steered" });
    expect(useStore.getState().live.c1.notice).toBe("Added to what I am doing now.");
  });

  it("leaves the running answer alone when a message is added to it", async () => {
    useStore.setState({
      paired: true,
      active: "c1",
      live: { c1: { text: "Half an answer", notice: null, error: null, artifacts: [], notes: [], helper: false, running: true } },
    });
    stream.mockImplementation(async (_cmd: string, _args: unknown, on: (e: unknown) => void) => {
      on({ type: "chat", conversation_id: "c1" });
      on({ type: "steered" });
    });
    await useStore.getState().send("also this");
    const live = useStore.getState().live.c1;
    expect(live.text).toBe("Half an answer");
    expect(live.running).toBe(true);
    expect(live.notice).toBe("Added to what I am doing now.");
    // The answer still arrives on the first stream, which finds the turn where it was.
    onTurnEvent("c1", { type: "token", text: " and the rest" });
    expect(useStore.getState().live.c1.text).toBe("Half an answer and the rest");
  });
});

describe("a spoken message", () => {
  it("hands the answer to the voice session as it arrives and waits until it was heard", async () => {
    const said: string[] = [];
    const bridge: SpeechBridge = {
      current: () => 7,
      token: (g, t) => said.push(`${g}:${t}`),
      step: (g, v) => said.push(`${g}:step ${v}`),
      end: async (g) => {
        said.push(`${g}:end`);
        return { interrupted: false, heard: "" };
      },
      stopSpeaking: () => {},
      fail: (m) => said.push(`fail ${m}`),
    };
    setSpeechBridge(bridge);
    useStore.setState({ paired: true, active: "c1" });
    stream.mockImplementation(async (_c: string, _a: unknown, on: (e: unknown) => void) => {
      on({ type: "step_start", id: "s", verb: "searched", target: "x" });
      on({ type: "token", text: "Hi" });
    });
    await useStore.getState().send("hello", [], { spoken: true });
    expect(stream.mock.calls[0][1]).toMatchObject({ spoken: true });
    expect(said).toEqual(["7:step searched", "7:Hi", "7:end"]);
    setSpeechBridge(null);
  });

  it("tells the voice session when the turn failed, so the voice surface is not left waiting", async () => {
    const said: string[] = [];
    setSpeechBridge({
      current: () => 1,
      token: () => {},
      step: () => {},
      end: async () => ({ interrupted: false, heard: "" }),
      stopSpeaking: () => {},
      fail: (m) => said.push(m),
    });
    useStore.setState({ paired: true, active: "c1" });
    stream.mockRejectedValue("I can't reach your desktop.");
    await useStore.getState().send("hello", [], { spoken: true });
    expect(said).toEqual(["I can't reach your desktop."]);
    setSpeechBridge(null);
  });

  it("puts a plain line under the open chat", () => {
    useStore.setState({ active: "c1" });
    useStore.getState().tell("Voice stopped when you left the app.");
    expect(useStore.getState().live.c1.notice).toBe("Voice stopped when you left the app.");
  });
});

describe("questions from the desktop", () => {
  const permission = { type: "permission" as const, request: { id: "q1", summary: "Write to report.md", path: "report.md" } };

  it("opens a card once, however often it is sent, and closes it when the desktop says it is gone", () => {
    onTurnEvent("c1", permission);
    onTurnEvent("c1", permission);
    expect(useStore.getState().approvals).toHaveLength(1);
    onFeed({ name: "poiesis-remote-turn", payload: { conversation_id: "c1", event: { type: "permission", request: permission.request } } });
    expect(useStore.getState().approvals).toHaveLength(1);
    onFeed({ name: "poiesis-remote-turn", payload: { conversation_id: "c1", event: { type: "permission_gone", id: "q1" } } });
    expect(useStore.getState().approvals).toHaveLength(0);
  });

  it("answers with yes once or no, and the card goes at once", async () => {
    onTurnEvent("c1", permission);
    await useStore.getState().answer("q1", true);
    expect(call).toHaveBeenCalledWith("answer", { request_id: "q1", allow: true });
    expect(useStore.getState().approvals).toHaveLength(0);
  });

  it("says so when the question was no longer open", async () => {
    useStore.setState({ active: "c1" });
    onTurnEvent("c1", permission);
    desktop({ answer: new Error("That question is not open any more.") });
    call.mockImplementation(async (cmd: string) => {
      if (cmd === "answer") throw "That question is not open any more.";
      return null;
    });
    await useStore.getState().answer("q1", false);
    expect(useStore.getState().live.c1.notice).toBe("That question is not open any more.");
  });

  it("shows a question that arrives from another chat, with the chat it belongs to", () => {
    onFeed({ name: "poiesis-remote-turn", payload: { conversation_id: "other", event: { type: "permission", request: permission.request } } });
    expect(useStore.getState().approvals[0].conversationId).toBe("other");
  });

  it("ignores feed events it does not use", () => {
    onFeed({ name: "poiesis-agent-sub", payload: {} });
    onFeed({ name: "x" });
    onFeed(null);
    expect(useStore.getState().approvals).toHaveLength(0);
  });
});

describe("the feed", () => {
  it("listens, and listens again after a drop", async () => {
    vi.useFakeTimers();
    let n = 0;
    stream.mockImplementation(async () => {
      n++;
      if (n === 1) throw "dropped";
      await new Promise(() => {}); // the second feed stays open
    });
    startFeed();
    await vi.advanceTimersByTimeAsync(0);
    expect(n).toBe(1);
    await vi.advanceTimersByTimeAsync(3100);
    expect(n).toBe(2);
    stopFeed();
    vi.useRealTimers();
  });

  it("comes back to the front: connects again and refreshes", async () => {
    useStore.setState({ paired: true });
    desktop({ conversations: [row("z", 5)] });
    await useStore.getState().resume();
    expect(connect).toHaveBeenCalled();
    expect(useStore.getState().chats[0].id).toBe("z");
    stopFeed();
  });
});
