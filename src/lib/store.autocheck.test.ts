/**
 * `SEE-1`: the automatic check of an answer reaches the screen over the app event, because
 * it outlives the run's stream. The answer is found by its run, since until the turn is
 * finalized it still has its optimistic id. Marks arrive one by one, the count goes up with
 * them, and a deep check the person asked for stands over this one.
 */
import { beforeEach, describe, expect, it } from "vitest";
import type { AutoCheckEvent, VerifyCheck, VerifyClaim } from "./api";
import { applyAutoCheck, useAppStore } from "./store";
import { emptyThought } from "./thought";

const claim = (over: Partial<VerifyClaim> = {}): VerifyClaim => ({
  id: "s1",
  text: "Paris is the capital of France.",
  quote: "Paris is the capital of France.",
  source: null,
  importance: 2,
  kind: "fact",
  finding: "supported",
  p: 0.95,
  how: "fast",
  settled: true,
  fast: "supported",
  slow: null,
  reason: null,
  passages: [],
  decision: "d1",
  mark: null,
  ...over,
});

const base = { conversation_id: "c1", message_id: "m-db", run_id: "r1" };
const check = (over: Partial<VerifyCheck> = {}): VerifyCheck => ({
  message_id: "m-db",
  conversation_id: "c1",
  created_at: 1,
  claims: [claim()],
  note: null,
  skipped: 0,
  mode: "auto",
  summary: { sentences: 2, asked: 2, answered: 2, over: 0, pool: 3, sources: 1, factual: 0 },
  ...over,
});

const send = (e: AutoCheckEvent) => applyAutoCheck((fn) => useAppStore.setState(fn), useAppStore.getState, e);
const entry = (id: string) => useAppStore.getState().verifications[id];

beforeEach(() => {
  useAppStore.setState({
    conversations: [
      {
        id: "c1",
        title: "t",
        updatedAt: 0,
        messages: [
          // Streaming: still the optimistic id, but it knows its run.
          { id: "a-123", role: "assistant", text: "Paris is the capital of France.", createdAt: 1, thought: { runId: "r1", from: 0, back: [], thought: emptyThought() } },
        ],
      },
    ],
    activeConversationId: "c1",
    verifications: {},
  });
});

describe("the marks of a live check", () => {
  it("land under the answer's current id, found by its run", () => {
    send({ type: "progress", ...base, done: 1, total: 3 });
    expect(entry("a-123")).toMatchObject({ running: true, progress: { phase: "auto", done: 1, total: 3 } });
    expect(entry("m-db")).toBeUndefined();
  });

  it("add a mark at a time to a check that is still being made", () => {
    send({ type: "claim", ...base, claim: claim() });
    send({ type: "claim", ...base, claim: claim({ id: "s2", finding: "refuted" }) });
    const e = entry("a-123");
    expect(e.running).toBe(true);
    expect(e.check).toMatchObject({ mode: "auto", message_id: "a-123" });
    expect(e.check!.claims.map((c) => [c.id, c.finding])).toEqual([["s1", "supported"], ["s2", "refuted"]]);
    // A mark that arrives again replaces itself.
    send({ type: "claim", ...base, claim: claim({ finding: "insufficient" }) });
    expect(entry("a-123").check!.claims.map((c) => c.finding)).toEqual(["refuted", "insufficient"]);
  });

  it("end with the check the backend kept, and stop running", () => {
    send({ type: "progress", ...base, done: 2, total: 2 });
    send({ type: "done", ...base, check: check() });
    expect(entry("a-123")).toMatchObject({ running: false, progress: null, error: null });
    expect(entry("a-123").check).toMatchObject({ mode: "auto", message_id: "a-123" });
  });

  it("leave nothing behind when the backend kept nothing", () => {
    send({ type: "progress", ...base, done: 1, total: 1 });
    send({ type: "done", ...base, check: null });
    expect(entry("a-123")).toBeUndefined();
  });

  it("use the stored id once the turn has been finalized", () => {
    useAppStore.setState((s) => ({
      conversations: s.conversations.map((c) => ({
        ...c,
        messages: c.messages.map((m) => ({ ...m, id: "m-db" })),
      })),
    }));
    send({ type: "claim", ...base, claim: claim() });
    expect(entry("m-db")).toBeDefined();
  });

  it("fall back to the stored id when the answer is not on screen", () => {
    send({ type: "claim", ...base, run_id: "gone", claim: claim() });
    expect(entry("m-db")).toBeDefined();
  });
});

describe("a deep check the person asked for", () => {
  it("is not replaced by the automatic one, whichever finishes first", () => {
    useAppStore.setState({
      verifications: { "a-123": { running: false, progress: null, check: check({ mode: "deep", summary: null }), error: null } },
    });
    send({ type: "claim", ...base, claim: claim({ id: "s9" }) });
    send({ type: "done", ...base, check: check() });
    expect(entry("a-123").check!.mode).toBe("deep");
    // And one that is running now keeps its own progress.
    useAppStore.setState({
      verifications: { "a-123": { running: true, progress: { phase: "checking", done: 1, total: 4 }, check: null, error: null } },
    });
    send({ type: "progress", ...base, done: 1, total: 2 });
    expect(entry("a-123").progress).toEqual({ phase: "checking", done: 1, total: 4 });
  });
});
