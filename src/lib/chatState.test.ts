import { beforeEach, describe, expect, it } from "vitest";
import { chatNow, chatSince, chatState, runWaitsForYou } from "./chatState";
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
    liveRuns: {},
    pendingPermissions: [],
    permissionChats: {},
    permissionAgents: {},
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
    // Its own chat waits on something else (`PAR-11`).
    expect(chatState(at({ subRuns: { a: run("a", { status: "queued" }) } }), "child-a")).toBe("held");
    expect(chatState(at({ subRuns: { a: run("a", { waiting: true }) } }), "child-a")).toBe("held");
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

  it("says what a long tool call is writing, before thinking", () => {
    const c = conv("c1", { messages: [{ id: "m", role: "assistant", text: "", streaming: true } as never] });
    const live = { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false } as RunSummary;
    const activeRun = (writing: object) => ({ runId: "r", convId: "c1", thinking: "old", writing });
    const read = (writing: object) => chatNow(at({ conversations: [c], runSummaries: { c1: live }, liveRuns: { c1: activeRun(writing) } }), "c1");
    expect(read({ label: "AI in Marketing", words: 900 })).toBe("Writing “AI in Marketing”");
    expect(read({ label: null, words: 900 })).toBe("Writing");
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

describe("several chats at once (PAR-9, PAR-10)", () => {
  const live = { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false } as RunSummary;
  const going = (convId: string, over: object = {}) => ({ runId: `r-${convId}`, convId, startedAt: 5, thinking: "", ...over });
  const answering = (id: string) => conv(id, { messages: [{ id: "m", role: "assistant", text: "", streaming: true } as never] });
  const two = (over: Record<string, unknown> = {}) =>
    at({
      conversations: [answering("a"), answering("b")],
      runSummaries: { a: live, b: live },
      liveRuns: { a: going("a", { thinking: "hm" }), b: going("b", { startedAt: 9 }) },
      ...over,
    });

  it("reads each chat from its own run", () => {
    const s = two({ liveRuns: { a: going("a", { thinking: "hm" }), b: going("b", { writing: { label: null, words: 3 } }) } });
    expect(chatNow(s, "a")).toBe("Thinking");
    expect(chatNow(s, "b")).toBe("Writing");
    expect(chatSince(two(), "a")).toBe(5);
    expect(chatSince(two(), "b")).toBe(9);
  });

  it("marks only the chat whose permission is open as waiting for you", () => {
    const s = two({ pendingPermissions: [{ id: "p", summary: "Run npm test?" }], permissionChats: { p: "b" } });
    expect(chatState(s, "b")).toBe("needs-you");
    expect(chatNow(s, "b")).toBe("Run npm test?");
    expect(chatState(s, "a")).toBe("running");
    expect(runWaitsForYou(s, "b")).toBe(true);
    expect(runWaitsForYou(s, "a")).toBe(false);
  });

  it("stands a run still for its own question, also while another chat's is first", () => {
    const s = two({
      pendingQuestion: question({ convId: "a", runId: "r-a", id: "qa" }),
      queuedQuestions: [question({ convId: "b", runId: "r-b", id: "qb", question: "Formal or casual?" })],
    });
    expect(runWaitsForYou(s, "a")).toBe(true);
    expect(runWaitsForYou(s, "b")).toBe(true);
    expect(chatNow(s, "b")).toBe("Formal or casual?");
  });

  it("says what a chat waits for while another holds its folder (PAR-5)", () => {
    const why = "Waiting: \u201cFix the login\u201d is changing files in this folder.";
    const s = two({ runSummaries: { a: live, b: { ...live, waitingOn: why } } });
    expect(chatState(s, "b")).toBe("held");
    expect(chatNow(s, "b")).toBe(why);
    expect(chatState(s, "a")).toBe("running");
  });
});
