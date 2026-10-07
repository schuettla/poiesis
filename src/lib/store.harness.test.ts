/**
 * `AGC-2`/`AGC-4`: what I offer, and what you do about it. One suggestion at a
 * time; a yes carries the change out in your session; a not now changes nothing
 * and is written down where I can learn from it (`CPX-5`).
 */
import { beforeEach, describe, expect, it } from "vitest";
import { useAppStore } from "./store";
import type { HarnessProposalView } from "./types";

beforeEach(() => {
  useAppStore.setState({
    conversations: [{ id: "c1", title: "Plan the trip", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    commandNotes: {},
    commandRequest: null,
    activeSuggestion: null,
    harnessProposals: [],
    turnModifiers: {},
    workspaceMode: false,
    toolsEnabled: false,
    taskDraft: null,
  });
});

const proposal = (over: Partial<HarnessProposalView>): HarnessProposalView => ({
  id: "p1",
  runId: "r1",
  convId: "c1",
  messageId: "m1",
  name: "switch_mode",
  reason: "this is a big change",
  payload: { mode: "workspace" },
  ...over,
});

const notes = () => useAppStore.getState().commandNotes.c1 ?? [];

describe("a suggestion (AGC-2)", () => {
  const suggest = () =>
    useAppStore.setState({
      activeSuggestion: { convId: "c1", runId: "r1", command: "skillify", reason: "That worked well" },
    });

  it("when accepted runs the command as if typed, and says you said yes", () => {
    suggest();
    useAppStore.getState().acceptSuggestion();
    const s = useAppStore.getState();
    expect(s.activeSuggestion).toBeNull();
    expect(s.commandRequest?.text).toBe("/skillify");
    expect(notes()).toHaveLength(1);
    expect(notes()[0]).toMatchObject({ name: "suggest", args: "skillify", by: "user", outcome: "accepted" });
  });

  it("when declined does nothing else, and is remembered as declined", () => {
    suggest();
    useAppStore.getState().dismissSuggestion();
    const s = useAppStore.getState();
    expect(s.activeSuggestion).toBeNull();
    expect(s.commandRequest).toBeNull();
    expect(notes()[0]).toMatchObject({ name: "suggest", args: "skillify", outcome: "declined" });
  });

  it("with none showing is a no-op", () => {
    useAppStore.getState().acceptSuggestion();
    useAppStore.getState().dismissSuggestion();
    expect(notes()).toEqual([]);
  });
});

describe("a proposal (AGC-4)", () => {
  it("to switch to Workspace, when accepted, turns it on with the tools it needs", async () => {
    useAppStore.setState({ harnessProposals: [proposal({})] });
    await useAppStore.getState().resolveHarnessProposal("p1", true);
    const s = useAppStore.getState();
    expect(s.workspaceMode).toBe(true);
    expect(s.toolsEnabled).toBe(true);
    expect(s.harnessProposals).toEqual([]);
    expect(notes()[0]).toMatchObject({ outcome: "accepted", by: "user" });
  });

  it("to plan first, when accepted, sets the chip for the next message only", async () => {
    useAppStore.setState({ harnessProposals: [proposal({ payload: { mode: "plan_first" } })] });
    await useAppStore.getState().resolveHarnessProposal("p1", true);
    expect(useAppStore.getState().turnModifiers).toEqual({ planFirst: true });
    expect(useAppStore.getState().workspaceMode).toBe(false);
  });

  it("when declined changes nothing and says so", async () => {
    useAppStore.setState({ harnessProposals: [proposal({})] });
    await useAppStore.getState().resolveHarnessProposal("p1", false);
    const s = useAppStore.getState();
    expect(s.workspaceMode).toBe(false);
    expect(s.turnModifiers).toEqual({});
    expect(s.harnessProposals).toEqual([]);
    expect(notes()[0]).toMatchObject({ outcome: "declined" });
    expect(notes()[0].note).toContain("not now");
  });

  it("to schedule opens a draft that says it, and saves nothing", async () => {
    useAppStore.setState({
      harnessProposals: [
        proposal({ name: "schedule", payload: { when: "every weekday at 9", task: "summarise my mail" } }),
      ],
    });
    await useAppStore.getState().resolveHarnessProposal("p1", true);
    const s = useAppStore.getState();
    expect(s.taskDraft).toMatchObject({ conversationId: "c1" });
    expect(s.taskDraft?.prompt).toContain("summarise my mail");
    // The time is read into the schedule field, not left in the task's words.
    expect(s.taskDraft?.prompt).toBe("summarise my mail");
    expect(s.taskDraft?.cadence).toBe("daily");
    expect(s.taskDraft?.whenNote).toContain("weekends");
    expect(s.view).toBe("tasks");
  });

  it("that is no longer there is ignored", async () => {
    await useAppStore.getState().resolveHarnessProposal("nope", true);
    expect(notes()).toEqual([]);
  });
});
