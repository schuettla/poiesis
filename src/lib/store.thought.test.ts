/**
 * `SEE-2`: what the quick-decision events do to the turn as they arrive. Driven
 * through the real stream handler: the strip data lands on the assistant turn, the
 * missing-engine line is kept to one per conversation, a group put back stops being
 * left out, and when the run ends the receipt's decisions are read from the log.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, DecisionEntry, Thought } from "./api";
import { emptyThought } from "./thought";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};
const runDecisions = vi.fn<(c: string, r: string, from: number, to: number) => Promise<DecisionEntry[]>>();
const receiptWrong = vi.fn<(id: string, value?: string) => Promise<boolean>>();
const restoreTools = vi.fn<(runId: string, units: string[]) => Promise<number>>();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  appendMessage: () => Promise.resolve({ id: "persisted-1" }),
  finalizeMessage: () => Promise.resolve(),
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  restoreTools: (runId: string, units: string[]) => restoreTools(runId, units),
  runDecisions: (c: string, r: string, from: number, to: number) => runDecisions(c, r, from, to),
  receiptWrong: (id: string, value?: string) => receiptWrong(id, value),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { keepThoughts, useAppStore } from "./store";
import type { Message } from "./types";

const started = (run = "r1"): AgentEvent => ({ type: "run_started", run_id: run, max_steps: 12, context_window: null });
const ended = (run = "r1"): AgentEvent => ({ type: "run_ended", run_id: run, stop_reason: "completed", steps: 1, ms: 5, usage: null, plan: null });
const said = (over: Partial<Thought>): AgentEvent => ({ type: "thought", run_id: "r1", thought: { ...emptyThought(), ...over } });
const WEB = { id: "web_search", words: "Not searching the web", action: "Search the web too" };

const thoughts = () =>
  useAppStore
    .getState()
    .conversations.find((c) => c.id === "c1")!
    .messages.map((m) => m.thought);
const last = () => {
  const all = thoughts().filter(Boolean);
  return all[all.length - 1]!;
};

const decision = (over: Partial<DecisionEntry>): DecisionEntry => ({
  id: "d1",
  conversation_id: "c1",
  run_id: "r1",
  purpose: "tool-select",
  label: "",
  background: false,
  experiment: "fast-slow",
  backend: "local:Clef-flash",
  model: "m",
  ms: 30,
  calibrated: false,
  answers: { web_search: { p: 0.02 } },
  input_tokens: 1,
  state_preview: "",
  action: "acted",
  at: 1,
  note: "Left out web search; you did not ask for it",
  outcomes: [],
  labels: [],
  ...over,
});

beforeEach(() => {
  runDecisions.mockReset().mockResolvedValue([]);
  receiptWrong.mockReset().mockResolvedValue(true);
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

describe("the strip on the turn", () => {
  it("keeps what the decisions understood on the assistant turn, with the run's id", async () => {
    script = (emit) => {
      emit(started());
      emit(said({ looking: ["on the web"], left_out: [WEB], ms: 38 }));
    };
    await useAppStore.getState().resumeLastRun();
    expect(last().runId).toBe("r1");
    expect(last().thought.looking).toEqual(["on the web"]);
    expect(last().thought.ms).toBe(38);
  });

  it("is an empty record at the start of a run, so the receipt can be read at its end", async () => {
    script = (emit) => emit(started());
    await useAppStore.getState().resumeLastRun();
    expect(last().thought).toEqual(emptyThought());
  });

  it("marks a group as back, from the model or from the person, and keeps the turn's record in step", async () => {
    script = (emit) => {
      emit(started());
      emit({ type: "tools_held", run_id: "r1", units: [{ id: "web_search", label: "Web search", p: 0.03 }] });
      emit(said({ direct: true, left_out: [WEB], held: [{ id: "web_search", tokens: 900 }] }));
      emit({ type: "tools_back", run_id: "r1", units: ["web_search"], by: "model" });
    };
    await useAppStore.getState().resumeLastRun();
    expect(last().back).toEqual(["web_search"]);
  });

  it("a click on a left-out group puts only that group back, at once, and tells the backend", async () => {
    script = (emit) => {
      emit(started());
      emit({
        type: "tools_held",
        run_id: "r1",
        units: [
          { id: "web_search", label: "Web search", p: 0.03 },
          { id: "apps", label: "Connected apps", p: 0.04 },
        ],
      });
      emit(said({ direct: true }));
      useAppStore.getState().restoreHeldTools("c1", ["web_search"]);
    };
    await useAppStore.getState().resumeLastRun();
    expect(restoreTools).toHaveBeenCalledWith("r1", ["web_search"]);
    expect(last().back).toEqual(["web_search"]);
  });
});

describe("the missing engine line", () => {
  it("is kept on the first turn that said it and dropped from the later ones in the conversation", async () => {
    script = (emit) => {
      emit(started());
      emit(said({ engine: "cold" }));
    };
    await useAppStore.getState().resumeLastRun();
    script = (emit) => {
      emit(started("r2"));
      emit({ type: "thought", run_id: "r2", thought: { ...emptyThought(), engine: "cold" } });
    };
    await useAppStore.getState().resumeLastRun();
    const engines = thoughts().filter(Boolean).map((t) => t!.thought.engine);
    expect(engines.filter(Boolean)).toHaveLength(1);
    expect(engines[engines.length - 1]).toBeNull();
  });
});

describe("the receipt's decisions", () => {
  it("are read from the log when the run ends, by this run's id and the time it ran", async () => {
    runDecisions.mockResolvedValue([decision({})]);
    script = (emit) => {
      emit(started());
      emit(said({ direct: true }));
      emit(ended());
    };
    await useAppStore.getState().resumeLastRun();
    expect(runDecisions).toHaveBeenCalledTimes(1);
    const [conv, run, from, to] = runDecisions.mock.calls[0];
    expect([conv, run]).toEqual(["c1", "r1"]);
    expect(to).toBeGreaterThan(from);
    await vi.waitFor(() => expect(last().decisions).toHaveLength(1));
    expect(last().to).toBe(to);
  });

  it("are not read while the run is still going", async () => {
    script = (emit) => emit(started());
    await useAppStore.getState().resumeLastRun();
    expect(runDecisions).not.toHaveBeenCalled();
  });

  it("show a row as marked wrong at once, and the label is written once for the receipt", async () => {
    runDecisions.mockResolvedValue([decision({})]);
    script = (emit) => {
      emit(started());
      emit(ended());
    };
    await useAppStore.getState().resumeLastRun();
    await vi.waitFor(() => expect(last().decisions).toHaveLength(1));
    await useAppStore.getState().markReceiptWrong("c1", "r1", "d1", ["web_search"]);
    expect(receiptWrong).toHaveBeenCalledWith("d1", "web_search");
    expect(last().decisions![0].labels).toEqual([{ label: "wrong", value: null, source: "receipt" }]);
  });
});

describe("a chat read again from the database", () => {
  const msg = (id: string, over: Partial<Message> = {}): Message => ({ id, role: "assistant", text: "", ...over }) as Message;
  const t = { runId: "r1", from: 1, back: [], thought: emptyThought() };

  it("keeps the receipts this session had, so switching away and back does not take them off", () => {
    const out = keepThoughts([msg("a", { thought: t }), msg("b")], [msg("a"), msg("b"), msg("c")]);
    expect(out.map((m) => m.thought)).toEqual([t, undefined, undefined]);
  });

  it("changes nothing when there was nothing to keep", () => {
    const loaded = [msg("a")];
    expect(keepThoughts([msg("a")], loaded)).toBe(loaded);
  });
});
