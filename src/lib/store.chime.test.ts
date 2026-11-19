/**
 * `TEAM-18`: after a reply in a session a team leads, the window asks who (if anyone) would add something
 * and, when someone would, streams that reply as a turn with no bubble of the person's. Driven through the
 * real stream handler with the backend's two calls scripted.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, TurnInput } from "./api";

const calls = vi.hoisted(() => ({ turns: [] as TurnInput[], asked: 0, next: [] as { role: string; nudge: string }[] }));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  teamNextVoice: async () => {
    calls.asked++;
    // What the backend would say: the rule gives one voice for each message of the person.
    return calls.next.shift() ?? null;
  },
  runTurn: async (input: TurnInput, onEvent: (e: AgentEvent) => void) => {
    calls.turns.push(input);
    onEvent({ type: "turn_saved", user_message_id: null, assistant_message_id: "real-a" });
    onEvent({ type: "token", text: "Juno: the button is too small." });
  },
}));

import { chimeIn, useAppStore } from "./store";

const reply = (over: Record<string, unknown> = {}) => ({
  id: "m-1",
  role: "assistant" as const,
  text: "Kai: the copy is fine.",
  steps: [],
  author: "persona:kai",
  createdAt: 1,
  ...over,
});

function put(leader: string, messages: unknown[]) {
  const model = useAppStore.getState().models[0];
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages, leader, personaId: null }],
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    toolsEnabled: false,
    turnModifiers: {},
    runSummaries: {},
    busy: false,
    presence: "idle",
  } as never);
}

const messages = () => useAppStore.getState().conversations[0].messages;

beforeEach(() => {
  calls.turns = [];
  calls.asked = 0;
  calls.next = [];
});

describe("an unasked reply (TEAM-18)", () => {
  it("is streamed as a turn of its own, from the member named, with no user message and nothing saved ahead", async () => {
    put("team:open", [{ id: "u-1", role: "user", text: "Is the page fine?", steps: [], createdAt: 0 }, reply()]);
    calls.next = [{ role: "juno", nudge: "Nobody asked you." }];
    await chimeIn(useAppStore.getState, "c1");

    expect(calls.turns).toHaveLength(1);
    expect(calls.turns[0]).toMatchObject({ conversationId: "c1", text: "Nobody asked you.", options: { addressed: "juno", unasked: true } });
    expect(calls.turns[0].saved).toBeUndefined();
    // One more reply on screen, and no user bubble for it.
    expect(messages().map((m) => m.role)).toEqual(["user", "assistant", "assistant"]);
    expect(messages()[2].text).toBe("Juno: the button is too small.");
    expect(useAppStore.getState().busy).toBe(false);
    // After that reply the chain asks once more and the backend, by its rule, says nobody.
    expect(calls.asked).toBe(2);
  });

  it("does nothing when the backend says nobody", async () => {
    put("team:open", [reply()]);
    await chimeIn(useAppStore.getState, "c1");
    expect(calls.asked).toBe(1);
    expect(calls.turns).toHaveLength(0);
  });

  it("does not even ask in a chat no team leads", async () => {
    put("poiesis", [reply()]);
    calls.next = [{ role: "juno", nudge: "x" }];
    await chimeIn(useAppStore.getState, "c1");
    expect(calls.asked).toBe(0);
    expect(calls.turns).toHaveLength(0);
  });

  it("does not ask after a reply that was stopped, or one with nothing in it, or while a run is going", async () => {
    calls.next = [{ role: "juno", nudge: "x" }];
    put("team:open", [reply({ stopReason: "aborted" })]);
    await chimeIn(useAppStore.getState, "c1");
    put("team:open", [reply({ text: "  " })]);
    await chimeIn(useAppStore.getState, "c1");
    put("team:open", [reply()]);
    useAppStore.setState({ busy: true } as never);
    await chimeIn(useAppStore.getState, "c1");
    expect(calls.asked).toBe(0);
    expect(calls.turns).toHaveLength(0);
  });
});
