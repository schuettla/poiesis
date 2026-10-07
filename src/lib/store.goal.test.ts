/**
 * `GOL-T1`, `GOL-3`: the goal loop as the real store runs it. Driven through the
 * stream handler by scripting a resumed run's events, so the end of the lead's own
 * turn is what decides whether to go on; nothing an agent says can start a goal.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, GoalCheck } from "./api";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};
let verdict: GoalCheck | Error = { met: false, evidence: "", next: "2 tests still fail" };
const stopChat = vi.fn(() => Promise.resolve());
let inDesktop = true;

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => inDesktop,
  appendMessage: () => Promise.resolve({ id: "persisted-1" }),
  finalizeMessage: () => Promise.resolve(),
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  stopChat: () => stopChat(),
  goalCheck: () => (verdict instanceof Error ? Promise.reject(verdict) : Promise.resolve(verdict)),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { useAppStore } from "./store";
import { newGoal } from "./goal";

const realSend = useAppStore.getState().sendMessage;
const sent: string[] = [];

beforeEach(() => {
  sent.length = 0;
  inDesktop = true;
  stopChat.mockClear();
  verdict = { met: false, evidence: "", next: "2 tests still fail" };
  const model = useAppStore.getState().models[0];
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    commandNotes: {},
    harnessProposals: [],
    activeSuggestion: null,
    pendingQuestion: null,
    workspaceMode: false,
    toolsEnabled: false,
    turnModifiers: {},
    busy: false,
    activeRun: null,
    presence: "idle",
    goals: {},
    // The next round is a real send; here it only has to be seen.
    sendMessage: (text: string) => {
      sent.push(text);
      return Promise.resolve();
    },
  });
});

const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null };
const ended = (stop_reason: "completed" | "aborted" | "error" | "max_steps" = "completed"): AgentEvent => ({
  type: "run_ended",
  run_id: "r1",
  stop_reason,
  steps: 2,
  ms: 10,
  usage: null,
  plan: null,
});

const goal = () => useAppStore.getState().goals.c1;
const settle = () => vi.waitFor(() => expect(goal()?.checking ?? false).toBe(false));

async function runOnce(finish: AgentEvent) {
  script = (emit) => {
    emit(started);
    emit(finish);
  };
  await useAppStore.getState().resumeLastRun();
  await settle();
  // The loop is not awaited by the run; give it the turns it needs.
  await new Promise((r) => setTimeout(r, 0));
}

describe("the end of a turn, with a goal set", () => {
  it("not met: says which round is next, then sends it", async () => {
    useAppStore.setState({ goals: { c1: newGoal("make the tests pass", "npm test exits 0") } });
    await runOnce(ended());
    expect(sent).toEqual(["Keep going toward our goal: make the tests pass. Last check: 2 tests still fail"]);
    expect(goal()).toMatchObject({ status: "active", round: 2 });
    const note = useAppStore.getState().commandNotes.c1.find((n) => n.name === "goal");
    expect(note).toMatchObject({ by: "agent", note: "Continuing toward our goal (round 2 of 5)" });
  });

  it("met: ends with the evidence and sends nothing", async () => {
    verdict = { met: true, evidence: "npm test exited 0", next: "" };
    useAppStore.setState({ goals: { c1: newGoal("make the tests pass", "npm test exits 0") } });
    await runOnce(ended());
    expect(sent).toEqual([]);
    expect(goal()).toMatchObject({ status: "met" });
    expect(goal().lastCheck?.evidence).toBe("npm test exited 0");
  });

  it("a run that was stopped ends the goal, and nothing is checked", async () => {
    useAppStore.setState({ goals: { c1: newGoal("g", "g") } });
    await runOnce(ended("aborted"));
    expect(sent).toEqual([]);
    expect(goal().status).toBe("stopped");
  });

  it("a run that failed before it could end ends the goal, and nothing is checked", async () => {
    // A model check would pass here; it must never be asked.
    verdict = { met: true, evidence: "looks done", next: "" };
    useAppStore.setState({ goals: { c1: newGoal("g", "g") } });
    await runOnce({ type: "error", message: "no model is running" });
    expect(sent).toEqual([]);
    expect(goal().status).toBe("stopped");
  });

  it("the last round ends it as not reached", async () => {
    useAppStore.setState({ goals: { c1: { ...newGoal("g", "g"), round: 5 } } });
    await runOnce(ended());
    expect(sent).toEqual([]);
    expect(goal().status).toBe("exhausted");
  });

  it("goes nowhere when you are looking at another chat", async () => {
    useAppStore.setState({
      goals: { c1: newGoal("g", "g") },
      conversations: [
        { id: "c1", title: "t", updatedAt: 0, messages: [] },
        { id: "c2", title: "u", updatedAt: 0, messages: [] },
      ],
    });
    script = (emit) => {
      emit(started);
      // The user moves to another chat before the run is over.
      useAppStore.setState({ activeConversationId: "c2" });
      emit(ended());
    };
    useAppStore.setState({ activeConversationId: "c1" });
    await useAppStore.getState().resumeLastRun();
    await new Promise((r) => setTimeout(r, 0));
    expect(sent).toEqual([]);
    expect(goal().status).toBe("stopped");
  });

  it("a chat with no goal behaves exactly as before", async () => {
    await runOnce(ended());
    expect(sent).toEqual([]);
    expect(useAppStore.getState().goals).toEqual({});
  });
});

describe("stopping", () => {
  it("Stop goal ends the goal and the turn running toward it", () => {
    useAppStore.setState({
      goals: { c1: newGoal("g", "g") },
      activeRun: {
        runId: "r1",
        convId: "c1",
        step: 1,
        maxSteps: 12,
        startedAt: 0,
        contextTokens: 0,
        contextWindow: null,
        thinking: "",
      },
    });
    useAppStore.getState().stopGoal("c1");
    expect(goal().status).toBe("stopped");
    expect(stopChat).toHaveBeenCalled();
  });

  it("Stop goal between rounds ends only the goal", () => {
    useAppStore.setState({ goals: { c1: newGoal("g", "g") } });
    useAppStore.getState().stopGoal("c1");
    expect(goal().status).toBe("stopped");
    expect(stopChat).not.toHaveBeenCalled();
  });

  it("the Stop button stops the goal too", () => {
    useAppStore.setState({ goals: { c1: newGoal("g", "g") } });
    useAppStore.getState().stopGenerating();
    expect(goal().status).toBe("stopped");
    expect(stopChat).toHaveBeenCalled();
  });

  it("stopping a goal that is over changes nothing", () => {
    useAppStore.setState({ goals: { c1: { ...newGoal("g", "g"), status: "met" } } });
    useAppStore.getState().stopGoal("c1");
    expect(goal().status).toBe("met");
  });
});

describe("who can start one", () => {
  it("startGoal makes it and sends the opening turn", async () => {
    await useAppStore.getState().startGoal("c1", "make the tests pass", "npm test exits 0");
    expect(goal()).toMatchObject({ status: "active", round: 1, maxRounds: 5 });
    expect(sent[0]).toContain("Goal: make the tests pass.");
  });

  it("nothing an agent does starts one", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "suggestion", run_id: "r1", command: "skillify", reason: "That worked well" });
      emit({
        type: "harness_proposal",
        run_id: "r1",
        id: "p1",
        name: "schedule",
        reason: "weekly",
        payload: { when: "every Monday", task: "keep going until the tests pass" },
      });
      emit({
        type: "harness_command",
        run_id: "r1",
        name: "compact",
        outcome: "done",
        note: "I made room.",
      });
      emit(ended());
    };
    await useAppStore.getState().resumeLastRun();
    await new Promise((r) => setTimeout(r, 0));
    expect(useAppStore.getState().goals).toEqual({});
    expect(sent).toEqual([]);
  });

  it("what a finished goal left on the bar goes with the next message", async () => {
    // The browser preview answers without a model, which is all this needs.
    inDesktop = false;
    useAppStore.setState({
      sendMessage: realSend,
      goals: { c1: { ...newGoal("g", "g"), status: "met" } },
    });
    await useAppStore.getState().sendMessage("something else");
    expect(useAppStore.getState().goals.c1).toBeUndefined();
  });
});
