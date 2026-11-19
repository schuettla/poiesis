/**
 * `TEAM-7`: Poiesis offering to hand a chat to someone. Driven through the real stream handler
 * by scripting a resumed run's events: the offer waits for the person, a yes hands the chat
 * over (and says so in the chat's own words), a no leaves everything as it was.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent } from "./api";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};
const setLeader = vi.fn();
const recorded = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  recordCommandTrace: (...a: unknown[]) => {
    recorded(...a);
    return Promise.resolve();
  },
  setConversationLeader: (...a: unknown[]) => setLeader(...a),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { useAppStore } from "./store";

const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null, tier: "full" };
const offer = (id: string): AgentEvent => ({
  type: "harness_proposal",
  run_id: "r1",
  id,
  name: "switch_mode",
  reason: "This looks like a job for Mira: it is writing.",
  payload: { mode: "persona:p1", leader: true, name: "Mira", auto: false },
});

beforeEach(() => {
  setLeader.mockReset().mockResolvedValue(undefined);
  recorded.mockReset();
  const model = useAppStore.getState().models[0];
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], leader: null, personaId: null }],
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    toolsEnabled: false,
    turnModifiers: {},
    runSummaries: {},
    harnessProposals: [],
    commandNotes: {},
    personas: [{ id: "p1", name: "Mira" }],
    busy: false,
    presence: "idle",
  } as never);
});

const notes = () => useAppStore.getState().commandNotes.c1?.map((n) => n.note) ?? [];

describe("an offer to hand the chat to someone (TEAM-7)", () => {
  it("waits for the person, and says what was asked in the first person", async () => {
    script = (emit) => {
      emit(started);
      emit(offer("p1"));
    };
    await useAppStore.getState().resumeLastRun();
    expect(useAppStore.getState().harnessProposals).toHaveLength(1);
    expect(setLeader).not.toHaveBeenCalled();
    expect(notes()).toEqual(["I asked to hand this chat to Mira: This looks like a job for Mira: it is writing."]);
  });

  it("hands the chat over on a yes, and says Mira leads it from here", async () => {
    script = (emit) => {
      emit(started);
      emit(offer("p1"));
    };
    await useAppStore.getState().resumeLastRun();
    await useAppStore.getState().resolveHarnessProposal("p1", true);
    expect(setLeader).toHaveBeenCalledWith("c1", "persona:p1");
    const chat = useAppStore.getState().conversations[0];
    expect(chat.leader).toBe("persona:p1");
    expect(chat.personaId).toBe("p1");
    expect(notes()[notes().length - 1]).toBe("Mira leads this chat from here.");
    // It is kept in the session log under the same name, which is how it is never offered twice.
    expect(recorded).toHaveBeenCalledWith("c1", expect.objectContaining({ name: "switch_mode", args: "persona:p1", outcome: "accepted" }));
  });

  it("leaves the chat with Poiesis on a no, and keeps the answer", async () => {
    script = (emit) => {
      emit(started);
      emit(offer("p2"));
    };
    await useAppStore.getState().resumeLastRun();
    await useAppStore.getState().resolveHarnessProposal("p2", false);
    expect(setLeader).not.toHaveBeenCalled();
    expect(useAppStore.getState().conversations[0].leader).toBeNull();
    expect(useAppStore.getState().harnessProposals).toHaveLength(0);
    expect(notes()[notes().length - 1]).toBe("you said not now to handing this chat to Mira");
    expect(recorded).toHaveBeenCalledWith("c1", expect.objectContaining({ name: "switch_mode", args: "persona:p1", outcome: "declined" }));
  });

  it("changes nothing when the persona is gone by the time of the yes", async () => {
    setLeader.mockRejectedValue("I don't know that persona.");
    script = (emit) => {
      emit(started);
      emit(offer("p3"));
    };
    await useAppStore.getState().resumeLastRun();
    await useAppStore.getState().resolveHarnessProposal("p3", true);
    expect(useAppStore.getState().conversations[0].leader).toBeNull();
    // And the chat says so, rather than that Mira leads it now.
    expect(notes()[notes().length - 1]).toBe("Mira is not here any more, so this chat stays as it was.");
  });
});
