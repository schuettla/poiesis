/**
 * `CMP-T4`: the chips are for one message. They become the backend's
 * `RunOptions`, and they are gone the moment that message is sent.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { turnRunOptions, useAppStore } from "./store";

beforeEach(() => {
  useAppStore.setState({
    turnModifiers: {},
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    busy: false,
    commandNotes: {},
  });
});

describe("turnRunOptions", () => {
  it("says nothing when there is nothing to say", () => {
    expect(turnRunOptions({})).toBeUndefined();
  });

  it("maps the chips onto the backend's names", () => {
    expect(turnRunOptions({ effort: "high", maxSteps: 30 })).toEqual({ effort: "high", maxSteps: 30 });
  });

  it("carries a named skill and its arguments", () => {
    expect(turnRunOptions({}, { skill: "review", skillArgs: "the parser" })).toEqual({
      skill: "review",
      skillArgs: "the parser",
    });
  });

  it("leaves out skill arguments that were not given", () => {
    expect(turnRunOptions({}, { skill: "init" })).toEqual({ skill: "init" });
  });
});

describe("the chips in the store", () => {
  it("set and clear one at a time", () => {
    const s = useAppStore.getState();
    s.setTurnModifier({ effort: "low" });
    s.setTurnModifier({ maxSteps: 5 });
    expect(useAppStore.getState().turnModifiers).toEqual({ effort: "low", maxSteps: 5 });
    useAppStore.getState().clearTurnModifier("effort");
    expect(useAppStore.getState().turnModifiers).toEqual({ maxSteps: 5 });
  });

  it("are cleared by the message they were for", async () => {
    useAppStore.getState().setTurnModifier({ effort: "high", maxSteps: 30 });
    await useAppStore.getState().sendMessage("hello");
    expect(useAppStore.getState().turnModifiers).toEqual({});
  });

  it("survive a send that never happened", async () => {
    useAppStore.setState({ busy: true });
    useAppStore.getState().setTurnModifier({ effort: "high" });
    await useAppStore.getState().sendMessage("hello");
    expect(useAppStore.getState().turnModifiers).toEqual({ effort: "high" });
  });
});

describe("recording a command", () => {
  it("keeps notes per conversation, oldest first", () => {
    const s = useAppStore.getState();
    s.recordCommand("c1", { name: "rename", args: "x", by: "user", outcome: "done" });
    s.recordCommand("c1", { name: "fork", args: "", by: "user", outcome: "done" });
    s.recordCommand(null, { name: "ignored", args: "", by: "user", outcome: "done" });
    const notes = useAppStore.getState().commandNotes.c1;
    expect(notes.map((n) => n.name)).toEqual(["rename", "fork"]);
    expect(Object.keys(useAppStore.getState().commandNotes)).toEqual(["c1"]);
  });
});
