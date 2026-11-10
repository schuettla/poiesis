/**
 * `WRK-10`, `WRK-12`, `WRK-13`: what done means, and switching how a run works in the
 * middle of it. Driven through the real stream handler by scripting a resumed run's
 * events, and looked at in the middle of the run.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, PlanView } from "./api";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};
const setStyle = vi.fn();
const switchRunStyle = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  setConversationStyle: (...a: unknown[]) => setStyle(...a),
  switchRunStyle: (...a: unknown[]) => switchRunStyle(...a),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { useAppStore, turnRunOptions } from "./store";

beforeEach(() => {
  setStyle.mockReset().mockImplementation(async (_id: string, preset: string | null) => preset);
  switchRunStyle.mockReset().mockResolvedValue(true);
  const model = useAppStore.getState().models[0];
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    toolsEnabled: false,
    turnModifiers: {},
    runSummaries: {},
    harnessProposals: [],
    presets: [
      { slug: "ad-hoc", name: "Ad hoc", line: "Just answer", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
      { slug: "mission-command", name: "Mission Command", line: "You set the goal, I find the way", glyph: "mission-command", built_in: true, locked: false, version: 1, rules: [] },
      { slug: "command-and-control", name: "Command & Control", line: "You decide every step", glyph: "command-and-control", built_in: true, locked: false, version: 1, rules: [] },
    ],
    busy: false,
    presence: "idle",
  });
});

const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null, tier: "full" };

describe("what done means (WRK-10, WRK-12)", () => {
  it("goes with the message that said it, and with nothing else", () => {
    expect(turnRunOptions({ style: "mission-command", criteria: ["npm test exits 0"] })).toEqual({
      style: "mission-command",
      criteria: ["npm test exits 0"],
    });
    expect(turnRunOptions({ criteria: [] })).toBeUndefined();
  });

  it("rides on the plan, so the card and the run bar can say how much is met", async () => {
    const plan: PlanView = {
      items: [],
      revisions: 0,
      awaiting_approval: false,
      criteria: [
        { text: "Fleet has an Export button", met: true },
        { text: "Excel opens it", met: false, why: "one column" },
      ],
    };
    script = (emit) => {
      emit(started);
      emit({ type: "plan", run_id: "r1", plan });
    };
    await useAppStore.getState().resumeLastRun();
    expect(useAppStore.getState().runSummaries.c1?.plan?.criteria?.map((c) => c.met)).toEqual([true, false]);
  });
});

describe("switching in the middle of a run (WRK-13)", () => {
  it("asks the run, says so on the chip until the run does it, and then says nothing", async () => {
    let during: unknown;
    let after: unknown;
    script = async (emit) => {
      emit(started);
      emit({ type: "style_set", run_id: "r1", preset: "mission-command", name: "Mission Command", source: "chat" });
      useAppStore.setState({ activeRun: { runId: "r1", convId: "c1" } as never });
      await useAppStore.getState().setConversationStyle("c1", "command-and-control");
      during = useAppStore.getState().runSummaries.c1?.switching;
      emit({ type: "style_set", run_id: "r1", preset: "command-and-control", name: "Command & Control", source: "switch" });
      after = useAppStore.getState().runSummaries.c1;
    };
    await useAppStore.getState().resumeLastRun();
    expect(switchRunStyle).toHaveBeenCalledWith("r1", "command-and-control");
    expect(during).toEqual({ to: "command-and-control", name: "Command & Control" });
    expect(after).toMatchObject({ style: { preset: "command-and-control" } });
    expect((after as { switching?: unknown }).switching).toBeUndefined();
  });

  it("does nothing to a run that already works that way, or when none is going", async () => {
    await useAppStore.getState().setConversationStyle("c1", "mission-command");
    expect(switchRunStyle).not.toHaveBeenCalled();
    script = async (emit) => {
      emit(started);
      emit({ type: "style_set", run_id: "r1", preset: "mission-command", name: "Mission Command", source: "chat" });
      useAppStore.setState({ activeRun: { runId: "r1", convId: "c1" } as never });
      await useAppStore.getState().setConversationStyle("c1", "mission-command");
    };
    await useAppStore.getState().resumeLastRun();
    expect(switchRunStyle).not.toHaveBeenCalled();
  });

  it("is carried out when the person says yes to an offer, and only then", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "harness_proposal",
        run_id: "r1",
        id: "p1",
        name: "switch_mode",
        reason: "This is bigger than it looked: 9 files so far.",
        payload: { mode: "mission-command", style: true, auto: false },
      });
    };
    await useAppStore.getState().resumeLastRun();
    const offered = useAppStore.getState().harnessProposals;
    expect(offered).toHaveLength(1);
    expect(setStyle).not.toHaveBeenCalled();

    await useAppStore.getState().resolveHarnessProposal("p1", true);
    expect(setStyle).toHaveBeenCalledWith("c1", "mission-command");
    expect(useAppStore.getState().conversations[0].style).toBe("mission-command");
  });

  it("keeps things as they are when the person says not now", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "harness_proposal",
        run_id: "r1",
        id: "p2",
        name: "switch_mode",
        reason: "This is bigger than it looked.",
        payload: { mode: "mission-command", style: true, auto: false },
      });
    };
    await useAppStore.getState().resumeLastRun();
    await useAppStore.getState().resolveHarnessProposal("p2", false);
    expect(setStyle).not.toHaveBeenCalled();
    expect(useAppStore.getState().harnessProposals).toHaveLength(0);
  });
});
