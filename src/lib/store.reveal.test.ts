/** `UI-3`: "Open the chat it came from" opens the lead's chat and names the turn that started the helper. */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "./store";
import type { SubRun } from "./types";

const run = { runId: "r1", conversationId: "child", parentConversationId: "lead", agent: "general", task: "t", status: "done", steps: [], text: "", startedAt: 0 } as SubRun;

describe("openTurnOf", () => {
  const openSession = vi.fn(async () => {});
  beforeEach(() => {
    openSession.mockClear();
    useAppStore.setState({
      openSession,
      revealMessageId: null,
      subRuns: { r1: run },
      conversations: [
        {
          id: "lead",
          title: "lead",
          updatedAt: 0,
          projectId: null,
          messages: [
            { id: "u1", role: "user", text: "do it" },
            { id: "a1", role: "assistant", text: "", subRunIds: ["r1"] },
          ],
        },
      ],
    } as never);
  });

  it("opens the parent chat and points at the turn whose helpers include the run", async () => {
    await useAppStore.getState().openTurnOf("r1");
    expect(openSession).toHaveBeenCalledWith("lead");
    expect(useAppStore.getState().revealMessageId).toBe("a1");
    useAppStore.getState().clearReveal();
    expect(useAppStore.getState().revealMessageId).toBeNull();
  });

  it("does nothing for a run it does not know", async () => {
    await useAppStore.getState().openTurnOf("nope");
    expect(openSession).not.toHaveBeenCalled();
  });
});
