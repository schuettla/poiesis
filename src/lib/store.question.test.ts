/**
 * `AGC-3`: a run waiting on a question hears the next thing typed as its
 * answer, and only one place decides that (`steerActiveRun`).
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const answerQuestion = vi.fn();
const steerRun = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  answerQuestion: (...a: unknown[]) => answerQuestion(...a),
  steerRun: (...a: unknown[]) => steerRun(...a),
  appendMessage: () => Promise.resolve({}),
}));

import { useAppStore } from "./store";

const question = {
  runId: "r1",
  id: "call_1",
  convId: "c1",
  messageId: "a-1",
  question: "Which database?",
  options: [{ label: "SQLite" }, { label: "Postgres" }],
  multi: false,
};

beforeEach(() => {
  answerQuestion.mockReset().mockResolvedValue(true);
  steerRun.mockReset().mockResolvedValue(true);
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
    pendingQuestion: question,
    presence: "listening",
    busy: true,
  });
});

describe("answering a question", () => {
  it("sends the choice to the run that asked, and clears the card", async () => {
    const delivered = await useAppStore.getState().answerQuestion({ choices: ["SQLite"] });
    expect(delivered).toBe(true);
    expect(answerQuestion).toHaveBeenCalledWith("r1", "call_1", { choices: ["SQLite"] });
    expect(useAppStore.getState().pendingQuestion).toBeNull();
    expect(useAppStore.getState().presence, "the mark stops listening").toBe("active");
  });

  it("says so when the run is gone, so the words are not lost", async () => {
    answerQuestion.mockResolvedValue(false);
    expect(await useAppStore.getState().answerQuestion({ choices: [], text: "either" })).toBe(false);
  });

  it("with nothing pending does nothing", async () => {
    useAppStore.setState({ pendingQuestion: null });
    expect(await useAppStore.getState().answerQuestion({ choices: ["x"] })).toBe(false);
    expect(answerQuestion).not.toHaveBeenCalled();
  });
});

describe("typing while a question is open", () => {
  it("answers it, and is not a steer", async () => {
    const delivered = await useAppStore.getState().steerActiveRun("Postgres, but small");
    expect(delivered).toBe(true);
    expect(answerQuestion).toHaveBeenCalledWith("r1", "call_1", { choices: [], text: "Postgres, but small" });
    expect(steerRun).not.toHaveBeenCalled();
    expect(
      useAppStore.getState().conversations[0].messages,
      "the answer is in the step row, not a stray user turn"
    ).toEqual([]);
  });

  it("is an ordinary steer when the question belongs to another chat", async () => {
    useAppStore.setState({ pendingQuestion: { ...question, convId: "other" } });
    await useAppStore.getState().steerActiveRun("look at the tests too");
    expect(steerRun).toHaveBeenCalledWith("r1", "look at the tests too");
    expect(answerQuestion).not.toHaveBeenCalled();
  });

  it("is an ordinary steer once the question is answered", async () => {
    await useAppStore.getState().answerQuestion({ choices: ["SQLite"] });
    answerQuestion.mockClear();
    await useAppStore.getState().steerActiveRun("and add tests");
    expect(steerRun).toHaveBeenCalledTimes(1);
    expect(answerQuestion).not.toHaveBeenCalled();
  });
});
