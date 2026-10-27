/**
 * `USE-5`, `USE-UI-2`, `USE-UI-3`: a message typed at a running agent is read by a
 * quick decision. Only a sure stop is held back, with a question under it. Every
 * other message goes on its way with its class as a tag, and both answers to the
 * question are labels. Reopening a ringed plan tick changes the stored plan.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const steerRun = vi.fn();
const steerAnswer = vi.fn();
const stopChat = vi.fn();
const reopenPlanItem = vi.fn();
const appendMessage = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  steerRun: (...a: unknown[]) => steerRun(...a),
  steerAnswer: (...a: unknown[]) => steerAnswer(...a),
  stopChat: (...a: unknown[]) => stopChat(...a),
  reopenPlanItem: (...a: unknown[]) => reopenPlanItem(...a),
  appendMessage: (...a: unknown[]) => appendMessage(...a),
}));

import { useAppStore } from "./store";
import type { Message } from "./types";

const messages = () => useAppStore.getState().conversations[0].messages;
const mine = () => messages().find((m) => m.role === "user")!;

beforeEach(() => {
  steerRun.mockReset().mockResolvedValue({ status: "queued" });
  steerAnswer.mockReset().mockResolvedValue(undefined);
  stopChat.mockReset().mockResolvedValue(undefined);
  reopenPlanItem.mockReset().mockResolvedValue(true);
  appendMessage.mockReset().mockResolvedValue({});
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
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
    pendingQuestion: null,
    goals: {},
    busy: true,
  });
});

describe("a message typed at a running agent", () => {
  it("keeps the class a quick decision read, as a tag, and sends it on", async () => {
    steerRun.mockResolvedValue({
      status: "queued",
      read: { kind: "add", p: 0.82, ask_stop: false, decision: "d1" },
    });
    expect(await useAppStore.getState().steerActiveRun("also add a README")).toBe(true);
    expect(mine().midRun).toBe("pending");
    expect(mine().steer).toEqual({ kind: "add", p: 0.82, decision: "d1" });
    expect(appendMessage).toHaveBeenCalledTimes(1);
  });

  it("shows no tag when the decision was not sure of any class", async () => {
    expect(await useAppStore.getState().steerActiveRun("look at the tests too")).toBe(true);
    expect(mine().steer).toBeUndefined();
    expect(mine().midRun).toBe("pending");
  });

  it("holds a sure stop under the message and still saves what was said", async () => {
    steerRun.mockResolvedValue({
      status: "held",
      read: { kind: "stop", p: 0.95, ask_stop: true, decision: "d2" },
    });
    expect(await useAppStore.getState().steerActiveRun("no wait, forget it")).toBe(true);
    expect(mine().midRun).toBe("held");
    expect(mine().steer?.decision).toBe("d2");
    expect(appendMessage).toHaveBeenCalledTimes(1);
    expect(stopChat, "nothing stops until the user says so").not.toHaveBeenCalled();
  });

  it("takes the message back when the run ended before it was sent", async () => {
    steerRun.mockResolvedValue({ status: "gone" });
    expect(await useAppStore.getState().steerActiveRun("too late")).toBe(false);
    expect(messages()).toEqual([]);
    expect(appendMessage).not.toHaveBeenCalled();
  });

  it("takes it back too when the send itself fails", async () => {
    steerRun.mockRejectedValue(new Error("no run"));
    expect(await useAppStore.getState().steerActiveRun("hello")).toBe(false);
    expect(messages()).toEqual([]);
  });
});

describe("answering Stop the run?", () => {
  async function held(): Promise<string> {
    steerRun.mockResolvedValueOnce({
      status: "held",
      read: { kind: "stop", p: 0.95, ask_stop: true, decision: "d2" },
    });
    await useAppStore.getState().steerActiveRun("forget it");
    return mine().id;
  }

  it("Stop labels the decision right and stops the run", async () => {
    const id = await held();
    await useAppStore.getState().answerSteerStop(id, true);
    expect(steerAnswer).toHaveBeenCalledWith("d2", true);
    expect(stopChat).toHaveBeenCalledTimes(1);
    expect(mine().midRun, "the question is gone").toBeUndefined();
    expect(steerRun, "it was never queued").toHaveBeenCalledTimes(1);
  });

  it("Keep going labels it wrong and sends the message on without reading it again", async () => {
    const id = await held();
    await useAppStore.getState().answerSteerStop(id, false);
    expect(steerAnswer).toHaveBeenCalledWith("d2", false);
    expect(stopChat).not.toHaveBeenCalled();
    expect(steerRun).toHaveBeenLastCalledWith("r1", "forget it", true);
    expect(mine().midRun, "waiting to be picked up like any steer").toBe("pending");
  });

  it("Keep going on a run that has ended leaves an ordinary unanswered turn", async () => {
    const id = await held();
    steerRun.mockResolvedValueOnce({ status: "gone" });
    await useAppStore.getState().answerSteerStop(id, false);
    expect(mine().midRun).toBeUndefined();
    expect(mine().text).toBe("forget it");
  });

  it("answers once: a second click on a settled message does nothing", async () => {
    const id = await held();
    await useAppStore.getState().answerSteerStop(id, true);
    await useAppStore.getState().answerSteerStop(id, true);
    expect(steerAnswer).toHaveBeenCalledTimes(1);
    expect(stopChat).toHaveBeenCalledTimes(1);
  });

  it("ignores a message that was never held", async () => {
    await useAppStore.getState().steerActiveRun("and add tests");
    await useAppStore.getState().answerSteerStop(mine().id, true);
    expect(steerAnswer).not.toHaveBeenCalled();
  });
});

describe("reopening a ringed plan tick", () => {
  const plan = (): Message["plan"] => ({
    items: [
      { text: "one", status: "done" },
      { text: "two", status: "done", judged: 0.9 },
    ],
    revisions: 0,
  });

  beforeEach(() => {
    useAppStore.setState({
      conversations: [
        {
          id: "c1",
          title: "t",
          updatedAt: 0,
          messages: [{ id: "m1", role: "assistant", text: "done", createdAt: 1, plan: plan() }],
        },
      ],
    });
  });

  it("opens that step again on screen once the stored plan has changed", async () => {
    await useAppStore.getState().reopenPlanItem("m1", 1);
    expect(reopenPlanItem).toHaveBeenCalledWith("m1", 1);
    const items = messages()[0].plan!.items;
    expect(items[1]).toMatchObject({ status: "todo", judged: null });
    expect(items[0], "the run's own tick is untouched").toMatchObject({ status: "done" });
  });

  it("changes nothing on screen when nothing was reopened", async () => {
    reopenPlanItem.mockResolvedValue(false);
    await useAppStore.getState().reopenPlanItem("m1", 1);
    expect(messages()[0].plan!.items[1]).toMatchObject({ status: "done", judged: 0.9 });
  });
});
