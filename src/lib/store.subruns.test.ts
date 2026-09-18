/**
 * `SUB-T9`: a delegated child's events fold into that child's own record, and
 * nowhere else.
 *
 * The second half is the one that matters. A child's steps arrive on the same
 * channel as the lead's, and if they landed in the lead's timeline the user
 * would read three agents' work as one agent's — which is exactly the
 * confusion the `Sub` wrapper exists to prevent.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { stillWorking } from "./api";
import { applySubEvent, useAppStore } from "./store";
import type { Conversation, SubRun } from "./types";

const CONV = "conv-lead";
const RUN = "run-child";

function child(overrides: Partial<SubRun> = {}): SubRun {
  return {
    runId: RUN,
    conversationId: "conv-child",
    parentConversationId: CONV,
    agent: "researcher",
    task: "read the docs",
    status: "running",
    steps: [],
    text: "",
    startedAt: 0,
    ...overrides,
  };
}

function lead(): Conversation {
  return {
    id: CONV,
    title: "A chat",
    updatedAt: 0,
    folderTrust: "confirm",
    messages: [
      { id: "a-1", role: "assistant", text: "", steps: [], streaming: true, createdAt: 0 },
    ],
  };
}

const set = useAppStore.setState;

beforeEach(() => {
  useAppStore.setState({
    conversations: [lead()],
    subRuns: { [RUN]: child() },
    pendingPermissions: [],
    permissionAgents: {},
  });
});

describe("a child's events", () => {
  it("build up that child's own steps and prose", () => {
    applySubEvent(set, RUN, { type: "step_start", id: "s1", verb: "read", target: "README.md" });
    applySubEvent(set, RUN, { type: "step_done", id: "s1", result: "— 40 lines" });
    applySubEvent(set, RUN, { type: "token", text: "It says " });
    applySubEvent(set, RUN, { type: "token", text: "hello." });

    const run = useAppStore.getState().subRuns[RUN];
    expect(run.steps).toHaveLength(1);
    expect(run.steps[0]).toMatchObject({ verb: "read", status: "done", result: "— 40 lines" });
    expect(run.text).toBe("It says hello.");
  });

  it("never land in the turn that started them", () => {
    applySubEvent(set, RUN, { type: "step_start", id: "s1", verb: "read", target: "README.md" });
    applySubEvent(set, RUN, { type: "token", text: "It says hello." });

    const message = useAppStore.getState().conversations[0].messages[0];
    expect(message.steps).toEqual([]);
    expect(message.text).toBe("");
  });

  it("carry the agent's name onto a permission prompt, and queue rather than replace", () => {
    const request = (id: string) => ({
      id,
      summary: "Read this file?",
      path: "C:\\work",
      mode: "read-only",
      in_folder: false,
    });
    applySubEvent(set, RUN, { type: "permission", request: request("p1") as never });
    applySubEvent(set, RUN, { type: "permission", request: request("p2") as never });

    const s = useAppStore.getState();
    expect(s.pendingPermissions.map((p) => p.id)).toEqual(["p1", "p2"]);
    expect(s.permissionAgents.p1).toBe("researcher");
  });

  it("are dropped for a run this session never saw, rather than inventing one", () => {
    applySubEvent(set, "run-unknown", { type: "token", text: "hi" });
    expect(Object.keys(useAppStore.getState().subRuns)).toEqual([RUN]);
  });

  /// `HRN-UI-2` reaches a child's own timeline too: a child that runs three
  /// reads at once should not read as three in a row in the Agents tab.
  it("carry a parallel batch onto the steps it announced", () => {
    applySubEvent(set, RUN, { type: "steps_parallel", ids: ["s1", "s2"] });
    applySubEvent(set, RUN, { type: "step_start", id: "s1", verb: "read", target: "a" });
    applySubEvent(set, RUN, { type: "step_start", id: "s2", verb: "read", target: "b" });
    applySubEvent(set, RUN, { type: "step_start", id: "s3", verb: "wrote", target: "c" });

    const steps = useAppStore.getState().subRuns[RUN].steps;
    expect(steps.map((s) => s.parallelGroup)).toEqual(["s1", "s1", undefined]);
  });

  /// `HRN-8`: the whole of a kept result reaches the user even though the model
  /// only ever saw its first couple of kilobytes.
  it("hang a kept result off the step that produced it", () => {
    applySubEvent(set, RUN, { type: "step_start", id: "s1", verb: "searched", target: "repo" });
    applySubEvent(set, RUN, {
      type: "kept_result",
      id: "s1",
      reference: "res_abc123",
      bytes: 42_000,
      text: "the whole thing",
    });
    const step = useAppStore.getState().subRuns[RUN].steps[0];
    expect(step.kept).toEqual({ reference: "res_abc123", bytes: 42_000, text: "the whole thing" });
  });

  /// `RPC-1`: a child can run a script that calls tools of its own. Those steps
  /// belong under the step that ran the script — forty of them arriving as
  /// ordinary rows would read as forty things the agent chose to do.
  it("hang a script's own tool calls under the step that ran it", () => {
    applySubEvent(set, RUN, { type: "step_start", id: "s1", verb: "worked out", target: "the numbers" });
    applySubEvent(set, RUN, { type: "step_start", id: "rpc_1", verb: "searched", target: "a", parent: "s1" });

    const steps = useAppStore.getState().subRuns[RUN].steps;
    expect(steps.map((s) => s.nestedUnder)).toEqual([undefined, "s1"]);
  });

  it("settle the steer mark once the child picks the instruction up", () => {
    useAppStore.setState({ subRuns: { [RUN]: child({ steerPending: true }) } });
    applySubEvent(set, RUN, { type: "steered", run_id: RUN, text: "also check the changelog" });
    expect(useAppStore.getState().subRuns[RUN].steerPending).toBe(false);
  });

  /// `SUB-12`: a background child sits in a queue first. Nothing else tells the
  /// UI it got a slot, so a missed `run_started` would leave it reading as
  /// "waiting its turn" for the whole of its actual run.
  it("turn a queued child into a working one when it gets its slot", () => {
    useAppStore.setState({ subRuns: { [RUN]: child({ status: "queued", startedAt: 0 }) } });
    applySubEvent(set, RUN, { type: "run_started", run_id: RUN, max_steps: 8, context_window: null });

    const run = useAppStore.getState().subRuns[RUN];
    expect(run.status).toBe("running");
    expect(run.startedAt).toBeGreaterThan(0);
  });
});

/// `SUB-10`: queued and working are one thing to every surface — neither is a
/// result. The bug this guards is a waiting agent rendering as a finished one,
/// which would show "done" under a child that has not run a single step.
describe("whether a child still has work to come", () => {
  it("counts a queued child as one that has not answered yet", () => {
    expect(stillWorking("queued")).toBe(true);
    expect(stillWorking("running")).toBe(true);
    for (const done of ["done", "stopped", "error"] as const) {
      expect(stillWorking(done)).toBe(false);
    }
  });
});
