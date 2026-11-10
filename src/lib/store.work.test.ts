/**
 * `WRK-1`, `WRK-2`, `WRK-3`: what a way of working does to the screen as it
 * arrives. Driven through the real stream handler by scripting a resumed run's
 * events, and looked at in the middle of the run, not only after it.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent } from "./api";

type Script = (emit: (e: AgentEvent) => void, look: () => void) => void | Promise<void>;
let script: Script = () => {};
const setStyle = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  setConversationStyle: (...a: unknown[]) => setStyle(...a),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent, () => {});
    return true;
  },
}));

import { useAppStore, turnRunOptions } from "./store";

type Look = {
  stopped: string | null;
  style: string | null;
  dividers: string[];
  notes: (string | undefined)[];
  looking: number | null;
  branching: number | null;
  pulsed: boolean;
  fresh: boolean;
};
const looks: Look[] = [];

function look() {
  const s = useAppStore.getState();
  const msgs = s.conversations.find((c) => c.id === "c1")?.messages ?? [];
  looks.push({
    stopped: s.runSummaries.c1?.stopped?.check ?? null,
    style: s.runSummaries.c1?.style?.name ?? null,
    dividers: msgs.flatMap((m) => (m.steps ?? []).filter((x) => x.divider).map((x) => x.divider as string)),
    notes: msgs.flatMap((m) => (m.steps ?? []).filter((x) => x.divider).map((x) => x.note)),
    looking: s.runSummaries.c1?.looking?.item ?? null,
    branching: s.runSummaries.c1?.branching?.item ?? null,
    pulsed: !!s.runSummaries.c1?.pulsedAt,
    fresh: !!s.runSummaries.c1?.freshAt,
  });
}

beforeEach(() => {
  looks.length = 0;
  setStyle.mockReset().mockImplementation(async (_id: string, preset: string | null) => preset);
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
    busy: false,
    presence: "idle",
  });
});

const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null, tier: "full" };

describe("the line (WRK-1)", () => {
  it("is stopped by a failing check, and starts again when it passes", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "line_stopped", run_id: "r1", check: "cargo test", rule: "stop-the-line", text: "When a check fails…" });
      look();
      emit({ type: "line_cleared", run_id: "r1", check: "cargo test" });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].stopped).toBe("cargo test");
    expect(looks[1].stopped).toBeNull();
  });

  it("is not carried past the end of the run", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "line_stopped", run_id: "r1", check: "npm test", rule: "stop-the-line", text: "…" });
      emit({
        type: "run_ended",
        run_id: "r1",
        stop_reason: "completed",
        steps: 3,
        ms: 10,
        usage: null,
        plan: null,
      });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].stopped, "a live state is gone with the run").toBeNull();
  });
});

describe("items (WRK-2)", () => {
  it("leave a divider where each one ended, in the order they happened", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "step_start", id: "s1", verb: "read", target: "a.ts" });
      emit({ type: "step_done", id: "s1", result: null });
      emit({ type: "item_ended", run_id: "r1", item: 0, text: "Export function", outcome: "done", why: null });
      emit({ type: "item_ended", run_id: "r1", item: 1, text: "Button", outcome: "dropped", why: "not needed" });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].dividers).toEqual(["Done: Export function", "Dropped: Button"]);
  });
});

describe("the second look (WRK-4)", () => {
  it("is on the run while someone is looking, and gone when they are done", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "review", run_id: "r1", item: 1, state: "checking", reason: null });
      look();
      emit({ type: "review", run_id: "r1", item: 1, state: "sent_back", reason: "no test" });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].looking).toBe(1);
    expect(looks[1].looking).toBeNull();
  });

  it("is not carried past the end of the run", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "review", run_id: "r1", item: 0, state: "checking", reason: null });
      emit({ type: "branched", run_id: "r1", item: 0, id: "b1" });
      emit({ type: "run_ended", run_id: "r1", stop_reason: "completed", steps: 1, ms: 5, usage: null, plan: null });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].looking).toBeNull();
    expect(looks[0].branching).toBeNull();
  });
});

describe("a fresh start (WRK-5)", () => {
  it("leaves a line that carries the note, and the mark is told", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "step_start", id: "s1", verb: "read", target: "a.ts" });
      emit({ type: "step_done", id: "s1", result: null });
      emit({ type: "fresh_start", run_id: "r1", note: "I am building the export." });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].dividers).toEqual(["Fresh start: I wrote myself a handover note"]);
    expect(looks[0].notes).toEqual(["I am building the export."]);
    expect(looks[0].fresh).toBe(true);
  });
});

describe("two ways (WRK-6)", () => {
  it("are tried on an item and then one is kept", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "branched", run_id: "r1", item: 2, id: "b1" });
      look();
      emit({ type: "branch_kept", run_id: "r1", item: 2, id: "b1", kept: "b", reason: "smaller" });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].branching).toBe(2);
    expect(looks[1].branching).toBeNull();
  });

  it("an item ending pulses the mark", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "item_ended", run_id: "r1", item: 0, text: "One", outcome: "done", why: null });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].pulsed).toBe(true);
  });
});

describe("how this run works (WRK-3)", () => {
  it("is kept on the run's summary for the board", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "style_set", run_id: "r1", preset: "lean", name: "Lean", source: "chat" });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(looks[0].style).toBe("Lean");
  });
});

describe("choosing how a chat works", () => {
  it("keeps what the backend kept, and nothing gives the choice back to the defaults", async () => {
    await useAppStore.getState().setConversationStyle("c1", "lean");
    expect(useAppStore.getState().conversations[0].style).toBe("lean");
    await useAppStore.getState().setConversationStyle("c1", null);
    expect(setStyle).toHaveBeenLastCalledWith("c1", null);
    expect(useAppStore.getState().conversations[0].style).toBeNull();
  });

  it("sends a preset for one message with that message, and nothing else changes", () => {
    expect(turnRunOptions({ style: "lean" })).toEqual({ style: "lean" });
    expect(turnRunOptions({})).toBeUndefined();
  });
});
