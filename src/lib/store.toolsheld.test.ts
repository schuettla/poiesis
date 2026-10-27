/**
 * `USE-4`, `USE-UI-1`: what the tool-menu events do to the run summary as they
 * arrive. Driven through the real stream handler, and looked at in the middle of
 * the run.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, HeldTool } from "./api";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};
const restoreTools = vi.fn<(runId: string, units: string[]) => Promise<number>>();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  appendMessage: () => Promise.resolve({ id: "persisted-1" }),
  finalizeMessage: () => Promise.resolve(),
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  restoreTools: (runId: string, units: string[]) => restoreTools(runId, units),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { useAppStore } from "./store";

const HELD: HeldTool[] = [
  { id: "web_search", label: "Web search", p: 0.03 },
  { id: "apps", label: "Connected apps", p: 0.04 },
];
const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null };
const held = () => useAppStore.getState().runSummaries.c1?.held;

beforeEach(() => {
  restoreTools.mockReset().mockResolvedValue(1);
  const model = useAppStore.getState().models[0];
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    runSummaries: {},
    toolsEnabled: false,
    turnModifiers: {},
    busy: false,
    presence: "idle",
  });
});

describe("the tool menu on the run summary (USE-UI-1)", () => {
  it("records what was held back, and nothing came back yet", async () => {
    let seen: ReturnType<typeof held>;
    script = (emit) => {
      emit(started);
      emit({ type: "tools_held", run_id: "r1", units: HELD });
      seen = held();
    };
    await useAppStore.getState().resumeLastRun();
    expect(seen!).toEqual({ units: HELD, back: [] });
  });

  it("marks a group as back when the model asked for it, without losing the list", async () => {
    let seen: ReturnType<typeof held>;
    script = (emit) => {
      emit(started);
      emit({ type: "tools_held", run_id: "r1", units: HELD });
      emit({ type: "tools_back", run_id: "r1", units: ["apps"], by: "model" });
      emit({ type: "tools_back", run_id: "r1", units: ["apps"], by: "you" });
      seen = held();
    };
    await useAppStore.getState().resumeLastRun();
    expect(seen!).toEqual({ units: HELD, back: ["apps"] });
  });

  it("a new run starts with a clean menu", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "tools_held", run_id: "r1", units: HELD });
    };
    await useAppStore.getState().resumeLastRun();
    expect(held()?.units).toHaveLength(2);
    script = (emit) => {
      emit({ ...started, run_id: "r2" });
    };
    await useAppStore.getState().resumeLastRun();
    expect(held()).toBeUndefined();
  });

  it("adding them back while the run is going tells the backend which groups and shows them back at once", async () => {
    script = async (emit) => {
      emit(started);
      emit({ type: "tools_held", run_id: "r1", units: HELD });
      emit({ type: "tools_back", run_id: "r1", units: ["apps"], by: "model" });
      useAppStore.getState().restoreHeldTools("c1");
    };
    await useAppStore.getState().resumeLastRun();
    expect(restoreTools).toHaveBeenCalledWith("r1", ["web_search"]);
    expect(held()?.back.sort()).toEqual(["apps", "web_search"]);
  });

  it("does nothing once the run is over", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "tools_held", run_id: "r1", units: HELD });
      emit({ type: "run_ended", run_id: "r1", stop_reason: "completed", steps: 1, ms: 5, usage: null, plan: null });
    };
    await useAppStore.getState().resumeLastRun();
    useAppStore.getState().restoreHeldTools("c1");
    expect(restoreTools).not.toHaveBeenCalled();
    expect(held()?.back).toEqual([]);
  });
});
