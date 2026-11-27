/**
 * `PAR-6`, `PAR-7`, `PAR-10`: two chats work at the same time. Each one's events change only
 * its own message, run and summary, and Stop, steering and answers go to the chat they mean.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent } from "./api";

const calls = vi.hoisted(() => ({
  streams: new Map<string, { emit: (e: AgentEvent) => void; end: () => void }>(),
  stopChat: vi.fn(() => Promise.resolve()),
  steerRun: vi.fn(() => Promise.resolve({ status: "queued" as const })),
  answerQuestion: vi.fn(() => Promise.resolve(true)),
  tellWaiting: vi.fn(() => Promise.resolve()),
}));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  appendMessage: () => Promise.resolve({ id: "persisted-1" }),
  finalizeMessage: () => Promise.resolve(),
  recordCommandTrace: () => Promise.resolve(),
  stopChat: calls.stopChat,
  steerRun: calls.steerRun,
  answerQuestion: calls.answerQuestion,
  tellWaiting: calls.tellWaiting,
  runDecisions: () => Promise.resolve([]),
  resolvePermission: () => Promise.resolve(),
  resumeRun: (conv: string, onEvent: (e: AgentEvent) => void) =>
    new Promise<boolean>((resolve) => {
      calls.streams.set(conv, { emit: onEvent, end: () => resolve(true) });
    }),
}));

import { useAppStore } from "./store";

const state = () => useAppStore.getState();
const turn = (id: string) =>
  state()
    .conversations.find((c) => c.id === id)!
    .messages.find((m) => m.role === "assistant")!;
const settle = () => new Promise((r) => setTimeout(r, 0));

/** Starts a turn in one chat and waits until its stream is open. */
async function start(convId: string) {
  useAppStore.setState({ activeConversationId: convId });
  const done = state().resumeLastRun();
  await settle();
  const stream = calls.streams.get(convId)!;
  stream.emit({ type: "run_started", run_id: `r-${convId}`, max_steps: 12, context_window: null, local: false });
  return { ...stream, done };
}

const finish = async (s: Awaited<ReturnType<typeof start>>, convId: string, reason: "completed" | "aborted" = "completed") => {
  s.emit({ type: "run_ended", run_id: `r-${convId}`, stop_reason: reason, steps: 1, ms: 1 } as AgentEvent);
  s.end();
  await s.done;
};

beforeEach(() => {
  calls.streams.clear();
  vi.clearAllMocks();
  const model = { ...state().models[0], provenance: "cloud" as const };
  useAppStore.setState({
    conversations: [
      { id: "a", title: "Plan the trip", updatedAt: 0, messages: [] },
      { id: "b", title: "Fix the login", updatedAt: 0, messages: [] },
    ],
    models: [model],
    selectedModelId: model.id,
    toolsEnabled: false,
    turnModifiers: {},
    busyChats: {},
    liveRuns: {},
    runSummaries: {},
    outcomes: {},
    pendingQuestion: null,
    queuedQuestions: [],
    pendingPermissions: [],
    permissionAgents: {},
    permissionChats: {},
    reflectingIds: [],
    presence: "idle",
    view: "chat",
  });
});

describe("two chats working at once", () => {
  it("keeps each chat's events to itself, and each end clears only its own chat", async () => {
    const a = await start("a");
    const b = await start("b");
    expect(Object.keys(state().busyChats).sort()).toEqual(["a", "b"]);

    a.emit({ type: "thinking", run_id: "r-a", text: "plan the days" });
    b.emit({ type: "token", text: "Looking at the login" });
    // B speaking does not end what A is thinking.
    expect(state().liveRuns.a.thinking).toBe("plan the days");
    expect(turn("b").text).toBe("Looking at the login");
    expect(turn("a").text).toBe("");

    b.emit({ type: "writing", run_id: "r-b", label: "notes", words: 10 });
    b.emit({ type: "step_start", id: "s1", verb: "reading", target: "auth.ts" });
    expect(state().liveRuns.b.writing).toBeUndefined();
    expect(turn("a").steps).toEqual([]);
    expect(state().runSummaries.a.runId).toBe("r-a");

    await finish(a, "a");
    expect(state().busyChats).toEqual({ b: "other" });
    expect(Object.keys(state().liveRuns)).toEqual(["b"]);
    expect(state().presence).toBe("active");
    expect(turn("b").streaming).toBe(true);

    await finish(b, "b", "aborted");
    expect(state().busyChats).toEqual({});
    expect(state().presence).toBe("idle");
    expect(state().outcomes).toEqual({ b: "aborted" });
  });

  it("does not send a second turn into a chat that works", async () => {
    const a = await start("a");
    await state().resumeLastRun();
    expect(state().conversations[0].messages).toHaveLength(1);
    await finish(a, "a");
  });

  it("stops and steers the chat that is meant", async () => {
    const a = await start("a");
    const b = await start("b");

    state().stopGenerating();
    expect(calls.stopChat).toHaveBeenLastCalledWith("b");
    state().stopGenerating("a");
    expect(calls.stopChat).toHaveBeenLastCalledWith("a");

    useAppStore.setState({ activeConversationId: "a" });
    await state().steerActiveRun("add Kyoto");
    expect(calls.steerRun).toHaveBeenLastCalledWith("r-a", "add Kyoto");

    await finish(a, "a");
    await finish(b, "b");
  });

  it("queues a second chat's question, answers each in its own chat, and says it elsewhere", async () => {
    const a = await start("a");
    const b = await start("b");
    const ask = (id: string) =>
      ({ type: "question", run_id: `r-${id}`, id: `q-${id}`, question: `${id}?`, options: [], multi: false }) as AgentEvent;

    a.emit(ask("a"));
    b.emit(ask("b"));
    expect(state().pendingQuestion?.id).toBe("q-a");
    expect(state().queuedQuestions.map((q) => q.id)).toEqual(["q-b"]);
    // B is on screen, so only A's question is said by the desktop.
    expect(calls.tellWaiting.mock.calls.map((c: unknown[]) => c[0])).toEqual(["a"]);

    // Typing in B answers B's question, though A's was asked first.
    await state().steerActiveRun("casual");
    expect(calls.answerQuestion).toHaveBeenLastCalledWith("r-b", "q-b", { choices: [], text: "casual" });
    expect(state().pendingQuestion?.id).toBe("q-a");
    expect(state().presence).toBe("listening");

    await finish(b, "b");
    expect(state().pendingQuestion?.id).toBe("q-a");
    await finish(a, "a");
    expect(state().pendingQuestion).toBeNull();
    expect(state().presence).toBe("idle");
  });

  it("keeps which chat a permission came from", async () => {
    const a = await start("a");
    const b = await start("b");
    a.emit({ type: "permission", request: { id: "p1", summary: "Run npm test?" } } as AgentEvent);
    expect(state().permissionChats).toEqual({ p1: "a" });
    await state().resolvePermission("p1", "once");
    expect(state().permissionChats).toEqual({});
    await finish(a, "a");
    await finish(b, "b");
  });

  it("says what a chat waits for until it has the folder", async () => {
    const a = await start("a");
    a.emit({ type: "held", run_id: "r-a", reason: "Waiting: \"Fix the login\" is changing files in this folder." });
    expect(state().runSummaries.a.waitingOn).toContain("Fix the login");
    a.emit({ type: "held", run_id: "r-a", reason: null });
    expect(state().runSummaries.a.waitingOn).toBeUndefined();
    await finish(a, "a");
  });
});
