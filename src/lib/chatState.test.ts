import { beforeEach, describe, expect, it } from "vitest";
import { chatNow, chatState } from "./chatState";
import { useAppStore } from "./store";
import type { ChangeProposal } from "./api";
import type { Conversation, PendingQuestion, RunSummary, SubRun } from "./types";

const conv = (id: string, over: Partial<Conversation> = {}): Conversation => ({
  id,
  title: id,
  updatedAt: 0,
  messages: [],
  projectId: null,
  ...over,
});

const run = (runId: string, over: Partial<SubRun> = {}): SubRun => ({
  runId,
  conversationId: `child-${runId}`,
  parentConversationId: "c1",
  agent: "general",
  task: "do it",
  status: "running",
  steps: [],
  text: "",
  startedAt: 0,
  ...over,
});

const question = (over: Partial<PendingQuestion> = {}): PendingQuestion => ({
  runId: "r1",
  id: "q1",
  convId: "c1",
  messageId: "m1",
  question: "Which table should I change?",
  options: [],
  multi: false,
  ...over,
});

const waitingEntry = (over: Partial<ChangeProposal> = {}): ChangeProposal =>
  ({ id: "w1", target: "waiting-ask", session_id: "c1", rationale: "api asks: Which table?", ...over }) as ChangeProposal;

function at(over: Record<string, unknown> = {}) {
  useAppStore.setState({
    conversations: [conv("c1")],
    waiting: [],
    pendingQuestion: null,
    queuedQuestions: [],
    runSummaries: {},
    subRuns: {},
    unreadIds: [],
    activeRun: null,
    ...over,
  } as never);
  return useAppStore.getState();
}

beforeEach(() => at());

describe("chatState (UI-1)", () => {
  it("is idle for a chat nothing happens in", () => {
    expect(chatState(at(), "c1")).toBe("idle");
  });

  it("says needs-you for something waiting, a pending question or a queued one", () => {
    expect(chatState(at({ waiting: [waitingEntry()] }), "c1")).toBe("needs-you");
    expect(chatState(at({ pendingQuestion: question() }), "c1")).toBe("needs-you");
    expect(chatState(at({ pendingQuestion: question({ convId: "other" }), queuedQuestions: [question()] }), "c1")).toBe("needs-you");
    expect(chatState(at({ pendingQuestion: question({ convId: "other" }) }), "c1")).toBe("idle");
  });

  it("says running for a live run or a helper that still works, queued included", () => {
    const live = { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false } as RunSummary;
    expect(chatState(at({ runSummaries: { c1: live } }), "c1")).toBe("running");
    expect(chatState(at({ subRuns: { a: run("a") } }), "c1")).toBe("running");
    expect(chatState(at({ subRuns: { a: run("a", { status: "queued" }) } }), "c1")).toBe("running");
    expect(chatState(at({ subRuns: { a: run("a", { status: "done" }) } }), "c1")).toBe("idle");
  });

  it("says failed or done-unseen only for a chat that changed while the person was away", () => {
    const failed = { runId: "r", startedAt: 0, live: false, files: [], costUsd: null, localRun: false, stopReason: "error" } as RunSummary;
    expect(chatState(at({ unreadIds: ["c1"], runSummaries: { c1: failed } }), "c1")).toBe("failed");
    expect(chatState(at({ runSummaries: { c1: failed } }), "c1")).toBe("idle");
    expect(chatState(at({ unreadIds: ["c1"] }), "c1")).toBe("done-unseen");
  });

  it("puts needs-you before running, running before failed, failed before done-unseen", () => {
    const live = { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false, stopReason: "error" } as RunSummary;
    const all = { waiting: [waitingEntry()], runSummaries: { c1: live }, unreadIds: ["c1"] };
    expect(chatState(at(all), "c1")).toBe("needs-you");
    expect(chatState(at({ ...all, waiting: [] }), "c1")).toBe("running");
    expect(chatState(at({ ...all, waiting: [], runSummaries: { c1: { ...live, live: false } } }), "c1")).toBe("failed");
  });

  it("reads a helper's own chat from its run, and a question an agent app asks is its", () => {
    const helper = run("a", { engine: { id: "claude", name: "Claude Code" } as never });
    expect(chatState(at({ subRuns: { a: helper } }), "child-a")).toBe("running");
    // The question carries the lead's chat and the helper's run.
    const asked = at({ subRuns: { a: helper }, pendingQuestion: question({ runId: "a", convId: "c1" }) });
    expect(chatState(asked, "child-a")).toBe("needs-you");
    expect(chatState(asked, "c1")).toBe("needs-you");
    expect(chatState(at({ subRuns: { a: helper, b: run("b") }, pendingQuestion: question({ runId: "a" }) }), "child-b")).toBe("running");
  });
});

describe("chatNow (UI-2)", () => {
  it("is nothing for a chat that neither works nor waits", () => {
    expect(chatNow(at(), "c1")).toBeNull();
    expect(chatNow(at({ unreadIds: ["c1"] }), "c1")).toBeNull();
  });

  it("follows the running step, then thinking, then the answer being written", () => {
    const streaming = (over: object) =>
      conv("c1", { messages: [{ id: "m", role: "assistant", text: "", streaming: true, ...over } as never] });
    const live = { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false } as RunSummary;
    const read = (c: Conversation, extra: Record<string, unknown> = {}) =>
      chatNow(at({ conversations: [c], runSummaries: { c1: live }, ...extra }), "c1");
    expect(
      read(streaming({ steps: [{ id: "s", verb: "searching web", target: "competitor pricing", status: "running" }] }))
    ).toBe("searching web competitor pricing");
    expect(read(streaming({}))).toBe("Thinking");
    expect(read(streaming({ text: "Here is" }))).toBe("Writing the answer");
    expect(read(conv("c1"))).toBe("Helpers working");
  });

  it("says the question, cut at a word, for a chat that waits", () => {
    const long = "Which of the three customer tables should I change, the old one or the one the import made last week?";
    const line = chatNow(at({ pendingQuestion: question({ question: long }) }), "c1")!;
    expect(line.endsWith("…")).toBe(true);
    expect(line.length).toBeLessThanOrEqual(61);
    expect(long.startsWith(line.slice(0, -1))).toBe(true);
    expect(long[line.length - 1]).toBe(" ");
    expect(chatNow(at({ waiting: [waitingEntry()] }), "c1")).toBe("api asks: Which table?");
  });

  it("says what a helper does from its own run", () => {
    const state = (r: SubRun) => at({ subRuns: { a: r } });
    expect(chatNow(state(run("a", { steps: [{ id: "s", verb: "reading", target: "a.ts", status: "running" } as never] })), "child-a")).toBe("reading a.ts");
    expect(chatNow(state(run("a", { text: "so far" })), "child-a")).toBe("Writing the answer");
    expect(chatNow(state(run("a")), "child-a")).toBe("Thinking");
    expect(chatNow(state(run("a", { status: "queued", queuedBecause: "Waiting: Codex is changing src/a.rs." })), "child-a")).toBe(
      "Waiting: Codex is changing src/a.rs."
    );
  });
});
