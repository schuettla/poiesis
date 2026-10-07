/**
 * `DEF-T2`, `PLF-T3`: Plan first as a default and as a one-message chip. Removing
 * the chip is "not this time" and never changes the setting; Go ahead never
 * carries the chip; the chip only ever shapes the message it was set for.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { turnRunOptions, useAppStore } from "./store";

const plan = {
  items: [{ text: "read the parser", status: "todo" as const }],
  revisions: 0,
  awaiting_approval: true,
};

// The real action, put back before each test: one of them swaps it for a spy.
const realSend = useAppStore.getState().sendMessage;

beforeEach(() => {
  useAppStore.setState({
    sendMessage: realSend,
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    turnModifiers: {},
    planFirstDefault: false,
    planRevising: false,
    busy: false,
    commandNotes: {},
  });
});

describe("what a message is sent with", () => {
  it("carries Plan first when the chip is on", () => {
    expect(turnRunOptions({ planFirst: true })).toEqual({ planFirst: true });
  });

  it("carries the approved plan and never the chip (PLF-T3)", () => {
    const sent = turnRunOptions({ planFirst: true, planFirstIsDefault: true }, { approvedPlan: plan });
    expect(sent).toEqual({ approvedPlan: plan });
    expect(sent?.planFirst, "approval that planned first again would never reach the work").toBeUndefined();
  });

  it("says nothing about the marker that only the chip's label needs", () => {
    expect(turnRunOptions({ planFirstIsDefault: true })).toBeUndefined();
  });
});

describe("Plan first by default (DEF-4, DEF-5)", () => {
  it("puts the chip on, marked as coming from the default", async () => {
    await useAppStore.getState().setPlanFirstDefault(true);
    expect(useAppStore.getState().turnModifiers).toEqual({ planFirst: true, planFirstIsDefault: true });
  });

  it("starts every message with the chip on again after one is sent", async () => {
    await useAppStore.getState().setPlanFirstDefault(true);
    await useAppStore.getState().sendMessage("hello");
    expect(useAppStore.getState().turnModifiers).toEqual({ planFirst: true, planFirstIsDefault: true });
  });

  it("removing the chip is not this time, and the setting is untouched", async () => {
    await useAppStore.getState().setPlanFirstDefault(true);
    useAppStore.getState().clearTurnModifier("planFirst");
    expect(useAppStore.getState().turnModifiers).toEqual({});
    expect(useAppStore.getState().planFirstDefault).toBe(true);
    // The next message is the one it was removed for; the one after has it back.
    await useAppStore.getState().sendMessage("hello");
    expect(useAppStore.getState().turnModifiers.planFirst).toBe(true);
  });

  it("turning the default off takes only the chip it put there", async () => {
    await useAppStore.getState().setPlanFirstDefault(true);
    await useAppStore.getState().setPlanFirstDefault(false);
    expect(useAppStore.getState().turnModifiers).toEqual({});

    useAppStore.setState({ turnModifiers: { planFirst: true } });
    await useAppStore.getState().setPlanFirstDefault(true);
    await useAppStore.getState().setPlanFirstDefault(false);
    expect(useAppStore.getState().turnModifiers, "one asked for by hand stays for this message").toEqual({
      planFirst: true,
    });
  });

  it("make default on the chip keeps it, and says so", async () => {
    useAppStore.setState({ turnModifiers: { planFirst: true } });
    await useAppStore.getState().makeModifierDefault("planFirst");
    const s = useAppStore.getState();
    expect(s.planFirstDefault).toBe(true);
    expect(s.turnModifiers.planFirstIsDefault).toBe(true);
    expect(s.commandNotes.c1?.[0]).toMatchObject({ name: "plan", note: "/plan is now my default" });
  });
});

describe("approving a plan (PLF-4)", () => {
  it("Go ahead sends the words and the plan, through the one send path", async () => {
    const send = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ sendMessage: send });
    await useAppStore.getState().approvePlan(plan);
    expect(send).toHaveBeenCalledWith("Go ahead with the plan.", [], { approvedPlan: plan });
  });

  it("Change something asks what, and keeps the next message read-only too", () => {
    useAppStore.getState().revisePlan();
    const s = useAppStore.getState();
    expect(s.planRevising).toBe(true);
    expect(s.turnModifiers.planFirst).toBe(true);
  });

  it("the revision is one message: after it is sent, the question and chip are gone", async () => {
    useAppStore.getState().revisePlan();
    await useAppStore.getState().sendMessage("drop the second step");
    const s = useAppStore.getState();
    expect(s.planRevising).toBe(false);
    expect(s.turnModifiers).toEqual({});
  });
});
