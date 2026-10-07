/**
 * `AGC-4`, `CPX-2`: what the agent's own acts do to the screen as they arrive.
 * Driven through the real stream handler by scripting a resumed run's events, and
 * looked at in the middle of the run, not only after it.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent } from "./api";

type Script = (emit: (e: AgentEvent) => void, look: () => void) => void | Promise<void>;
let script: Script = () => {};

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  appendMessage: () => Promise.resolve({ id: "persisted-1" }),
  finalizeMessage: () => Promise.resolve(),
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent, () => {});
    return true;
  },
}));

import { useAppStore } from "./store";

const snapshots: { notes: string[]; presence: string; question: boolean; proposals: number; suggestion: string | null }[] =
  [];

function look() {
  const s = useAppStore.getState();
  snapshots.push({
    notes: (s.commandNotes.c1 ?? []).map((n) => n.note ?? n.name),
    presence: s.presence,
    question: s.pendingQuestion !== null,
    proposals: s.harnessProposals.length,
    suggestion: s.activeSuggestion?.command ?? null,
  });
}

beforeEach(() => {
  snapshots.length = 0;
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
    presence: "idle",
  });
});

const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null };

describe("making room (CLR-3, CPX-2)", () => {
  it("puts a first-person note in the transcript and tends the mark, then lets it go", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "harness_command",
        run_id: "r1",
        name: "compact",
        outcome: "done",
        note: "I made room: I cleared 3 old results I no longer need word for word.",
      });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(snapshots[0].notes).toEqual(["I made room: I cleared 3 old results I no longer need word for word."]);
    expect(snapshots[0].presence).toBe("tending");
    const note = useAppStore.getState().commandNotes.c1[0];
    expect(note).toMatchObject({ by: "agent", name: "compact", outcome: "done" });
  });
});

describe("a proposal (AGC-4)", () => {
  it("waits for you and does not stop the run", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "harness_proposal",
        run_id: "r1",
        id: "call_1",
        name: "switch_mode",
        reason: "this is a big change",
        payload: { mode: "workspace", auto: false },
      });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(snapshots[0].proposals).toBe(1);
    expect(snapshots[0].notes[0]).toBe("I asked to switch to Workspace: this is a big change");
    expect(useAppStore.getState().workspaceMode, "nothing changes until you say so").toBe(false);
  });

  it("follows its turn to the turn's real id once the turn is saved", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "harness_proposal",
        run_id: "r1",
        id: "call_1",
        name: "schedule",
        reason: "you asked for this weekly",
        payload: { when: "every Monday", task: "send the report" },
      });
    };
    await useAppStore.getState().resumeLastRun();
    const kept = useAppStore.getState().harnessProposals;
    expect(kept).toHaveLength(1);
    expect(kept[0].messageId, "the optimistic id is gone after finalize").toBe("persisted-1");
  });

  it("at the auto rung is carried out for the next message, and says so", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "harness_proposal",
        run_id: "r1",
        id: "call_2",
        name: "switch_mode",
        reason: "this needs a surface",
        payload: { mode: "workspace", auto: true },
      });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(snapshots[0].proposals, "there is nothing to answer").toBe(0);
    expect(useAppStore.getState().workspaceMode).toBe(true);
    const said = (useAppStore.getState().commandNotes.c1 ?? []).map((n) => n.note ?? "").join(" | ");
    expect(said).toContain("as you let me");
  });
});

describe("a suggestion (AGC-2)", () => {
  it("is one chip at a time: a newer one replaces the older", async () => {
    script = (emit) => {
      emit(started);
      emit({ type: "suggestion", run_id: "r1", command: "skillify", reason: "That worked well" });
      emit({ type: "suggestion", run_id: "r1", command: "reflect", reason: "A lot went wrong" });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(snapshots[0].suggestion).toBe("reflect");
    expect(snapshots[0].notes).toEqual([
      "I suggested /skillify: That worked well",
      "I suggested /reflect: A lot went wrong",
    ]);
  });
});

describe("a question (AGC-3, CPX-2)", () => {
  it("makes the mark listen while the run waits, and is gone when the run ends", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "question",
        run_id: "r1",
        id: "call_3",
        question: "Which database?",
        options: [{ label: "SQLite" }, { label: "Postgres" }],
        multi: false,
      });
      look();
      emit({
        type: "run_ended",
        run_id: "r1",
        stop_reason: "aborted",
        steps: 2,
        ms: 10,
        usage: null,
        plan: null,
      });
      look();
    };
    await useAppStore.getState().resumeLastRun();
    expect(snapshots[0]).toMatchObject({ question: true, presence: "listening" });
    expect(snapshots[1].question, "a stopped run is not waiting on anyone").toBe(false);
    expect(snapshots[1].presence).not.toBe("listening");
  });
});
