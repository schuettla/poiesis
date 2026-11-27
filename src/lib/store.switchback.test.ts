/**
 * A run goes on while its chat is out of view. Coming back reads the chat from the
 * database, where the running turn's row is still empty, so the turn in memory has to win.
 */
import { describe, expect, it, vi } from "vitest";
import type { AgentEvent, DbMessage } from "./api";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};
const rows: DbMessage[] = [];

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  appendMessage: () => Promise.resolve({ id: "persisted-1" }),
  finalizeMessage: () => Promise.resolve(),
  recordCommandTrace: () => Promise.resolve(),
  setConversationWorkspace: () => Promise.resolve(),
  listMessages: (id: string) => Promise.resolve(rows.filter((r) => r.conversation_id === id)),
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { useAppStore } from "./store";

const turn = () =>
  useAppStore
    .getState()
    .conversations.find((c) => c.id === "c1")!
    .messages.find((m) => m.role === "assistant")!;

describe("leaving a chat while its run works, and coming back", () => {
  it("keeps the running turn instead of its empty row", async () => {
    const model = useAppStore.getState().models[0];
    useAppStore.setState({
      conversations: [
        { id: "c1", title: "t", updatedAt: 0, messages: [] },
        { id: "c2", title: "u", updatedAt: 0, messages: [] },
      ],
      activeConversationId: "c1",
      selectedModelId: model.id,
      engineReady: true,
      loadedModelId: model.id,
      runSummaries: {},
      toolsEnabled: false,
      turnModifiers: {},
      busyChats: {},
    });
    script = async (emit) => {
      emit({ type: "run_started", run_id: "r1", max_steps: 12, context_window: null });
      emit({ type: "token", text: "Half an answer" });
      rows.push({
        id: turn().id,
        conversation_id: "c1",
        role: "assistant",
        content: "",
        model_name: null,
        model_provenance: null,
        steps_json: null,
        stop_reason: null,
        plan_json: null,
        created_at: 1,
      });
      await useAppStore.getState().setActiveConversation("c2");
      await useAppStore.getState().setActiveConversation("c1");
      back = turn();
    };
    // Read here, checked below: the run handler would swallow a failed expectation.
    let back: ReturnType<typeof turn> | undefined;

    await useAppStore.getState().resumeLastRun();

    expect(back?.streaming).toBe(true);
    expect(back?.text).toBe("Half an answer");
  });
});

describe("a long tool call being written", () => {
  it("takes the place of the thinking, and goes when the call is made", async () => {
    const model = useAppStore.getState().models[0];
    useAppStore.setState({
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
      activeConversationId: "c1",
      selectedModelId: model.id,
      engineReady: true,
      loadedModelId: model.id,
      busyChats: {},
    });
    const seen: unknown[] = [];
    const run = () => useAppStore.getState().liveRuns.c1;
    script = (emit) => {
      emit({ type: "run_started", run_id: "r1", max_steps: 12, context_window: null });
      emit({ type: "thinking", run_id: "r1", text: "let me plan" });
      emit({ type: "writing", run_id: "r1", label: "AI in Marketing", words: 900 });
      seen.push({ thinking: run()?.thinking, writing: run()?.writing });
      emit({ type: "step_start", id: "s1", verb: "created", target: "AI in Marketing" });
      seen.push(run()?.writing);
    };

    await useAppStore.getState().resumeLastRun();

    expect(seen).toEqual([{ thinking: "", writing: { label: "AI in Marketing", words: 900 } }, undefined]);
  });
});
