import { describe, expect, it } from "vitest";
import type { Message, SubRun } from "../types";
import { activityFor, activitySummary } from "./activity";

const msg = (patch: Partial<Message>): Message => ({
  id: "a1",
  role: "assistant",
  text: "",
  createdAt: 2000,
  ...patch,
});

const run = (patch: Partial<SubRun>): SubRun => ({
  runId: "r1",
  conversationId: "c2",
  parentConversationId: "c1",
  agent: "researcher",
  task: "find the release date",
  status: "running",
  steps: [],
  text: "",
  startedAt: 2100,
  ...patch,
});

describe("activityFor", () => {
  it("lists steps with how each one is going", () => {
    const rows = activityFor(
      msg({
        steps: [
          { id: "s1", verb: "searched", target: "the web", status: "done", result: "— 3 results" },
          { id: "s2", verb: "read", target: "example.com", status: "error", result: "— not reachable" },
          { id: "s3", verb: "reading", target: "docs.rs", status: "running" },
        ],
      }),
      {},
      1000
    );
    expect(rows.map((r) => [r.label, r.state, r.detail])).toEqual([
      ["searched the web", "done", "3 results"],
      ["read example.com", "error", "not reachable"],
      ["reading docs.rs", "running", undefined],
    ]);
  });

  it("shows agents the turn started, with their own status", () => {
    const rows = activityFor(
      msg({ subRunIds: ["r1", "r2", "gone"] }),
      { r1: run({ status: "done" }), r2: run({ runId: "r2", agent: "coder", task: "fix it", status: "queued" }) },
      1000
    );
    expect(rows.map((r) => [r.kind, r.label, r.state])).toEqual([
      ["agent", "researcher: find the release date", "done"],
      ["agent", "coder: fix it", "running"],
    ]);
  });

  it("leaves out a child's own steps; the agent row covers them", () => {
    const rows = activityFor(
      msg({
        steps: [
          { id: "s1", verb: "delegated", target: "a search", status: "done" },
          { id: "s2", verb: "searched", target: "the web", status: "done", nestedUnder: "s1" },
        ],
      }),
      {},
      1000
    );
    expect(rows.map((r) => r.id)).toEqual(["s1"]);
  });

  it("ignores an answer from before the voice conversation began", () => {
    const old = msg({ createdAt: 500, steps: [{ id: "s1", verb: "searched", target: "x", status: "done" }] });
    expect(activityFor(old, {}, 1000)).toEqual([]);
    expect(activityFor(msg({ role: "user" }), {}, 1000)).toEqual([]);
    expect(activityFor(undefined, {}, 1000)).toEqual([]);
  });
});

describe("activitySummary", () => {
  it("says what is going on in a sentence", () => {
    const row = (state: "running" | "done" | "error") => ({ id: state, kind: "step" as const, label: "x", state });
    expect(activitySummary([row("done"), row("running")])).toBe("Working on 1 thing");
    expect(activitySummary([row("done"), row("error")])).toBe("1 thing did not work");
    expect(activitySummary([row("done"), row("done")])).toBe("Did 2 things");
  });
});
