/**
 * The Workspace's store logic: what a block or surface interaction sends and
 * remembers, how often a typed value is saved, and which chats are told about the
 * interface at all.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const saved: { id: string; json: string }[] = [];

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  updateBlockState: (id: string, json: string) => {
    saved.push({ id, json });
    return Promise.resolve();
  },
  setSessionState: () => Promise.resolve(),
}));

import { composeSystemPrompt, useAppStore } from "./store";
import type { Conversation } from "./types";

const conv = (workspace: boolean): Conversation =>
  ({ id: "c1", title: "t", updatedAt: 0, messages: [], workspace }) as never;

const surface = { id: "s1", kind: "surface", title: "Workspace", data: { type: "stack" } } as never;

describe("which chats are told about the interface", () => {
  const opts = (workspace: boolean) => ({
    conv: conv(workspace),
    sessionState: undefined,
    toolsEnabled: true,
    surface,
  });

  it("a chat outside Workspace mode hears nothing about composing one", () => {
    const out = composeSystemPrompt("Base.", opts(false));
    expect(out).not.toContain("render_ui");
    expect(out).not.toContain("Workspace surface");
    expect(out).toContain("Working through a task");
  });

  it("a chat in the mode is told how, and shown the one it made", () => {
    const out = composeSystemPrompt("Base.", opts(true));
    expect(out).toContain("Composing the workspace");
    expect(out).toContain("Workspace surface");
  });

  it("with tools off, neither is mentioned", () => {
    const out = composeSystemPrompt("Base.", { ...opts(true), toolsEnabled: false });
    expect(out).toBe("Base.");
  });
});

describe("an interaction is an ordinary turn", () => {
  const sent: { text: string; opts: unknown }[] = [];

  beforeEach(() => {
    sent.length = 0;
    const model = useAppStore.getState().models[0];
    useAppStore.setState({
      conversations: [conv(true)],
      activeConversationId: "c1",
      selectedModelId: model.id,
      workspaceMode: true,
      busy: false,
      sessionState: {},
      pendingActions: {},
      surfaces: { c1: { ...(surface as object), state: { pick: "a" } } as never },
      sendMessage: ((text: string, _a: unknown, opts: unknown) => {
        sent.push({ text, opts });
        return Promise.resolve();
      }) as never,
    });
  });

  it("goes through sendMessage with its sentence, and the action beside it", async () => {
    await useAppStore.getState().sendSurfaceAction("Chose “Next” in the workspace.", { action: "next" });
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toContain("Chose “Next” in the workspace.");
    expect(sent[0].text).toContain("```poiesis-action");
    expect(sent[0].text).toContain('"a":"ui_action"');
    expect(sent[0].text).toContain('"state":{"pick":"a"}');
    expect(sent[0].opts).toEqual({ action: "Chose “Next” in the workspace." });
  });

  it("a pin or a tick in Workspace mode only queues a note and spends no turn", async () => {
    await useAppStore.getState().sendBlockAction("b1", "Pinned “A”", { a: "block_action", action: "pin" });
    expect(sent).toHaveLength(0);
    expect(useAppStore.getState().pendingActions.c1).toEqual(["Pinned “A”"]);
  });

  it("does nothing while a turn is running", async () => {
    useAppStore.setState({ busy: true });
    await useAppStore.getState().sendSurfaceAction("Chose “Next”.", { action: "next" });
    expect(sent).toHaveLength(0);
  });
});

describe("what an interaction writes into the session state", () => {
  beforeEach(() => {
    const model = useAppStore.getState().models[0];
    useAppStore.setState({
      conversations: [conv(true)],
      activeConversationId: "c1",
      selectedModelId: model.id,
      workspaceMode: true,
      busy: false,
      sessionState: {},
      pendingActions: {},
      surfaces: { c1: surface },
      sendMessage: (() => Promise.resolve()) as never,
    });
  });

  it("a pinned option in a typed block becomes a decision", async () => {
    await useAppStore
      .getState()
      .sendBlockAction("b1", "Pinned “B”", { a: "block_action", action: "select", title: "B" });
    expect(useAppStore.getState().sessionState.c1).toEqual({ decisions: { selection: "B" } });
  });

  it("a composed surface naming its action `select` or `submit` writes nothing", async () => {
    await useAppStore
      .getState()
      .sendSurfaceAction("Selected “B” in the workspace.", { action: "select", item: "B" });
    await useAppStore.getState().sendSurfaceAction("Sent.", { action: "submit", data: { x: 1 } });
    expect(useAppStore.getState().sessionState.c1 ?? {}).toEqual({});
  });
});

describe("typing in a composed interface", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    saved.length = 0;
    useAppStore.setState({
      conversations: [conv(true)],
      activeConversationId: "c1",
      workspaceMode: true,
      busy: false,
      surfaces: { c1: surface },
      sendMessage: (() => Promise.resolve()) as never,
    });
  });
  afterEach(() => vi.useRealTimers());

  it("follows on screen at once and reaches the database once, after a pause", () => {
    const { setSurfaceState } = useAppStore.getState();
    for (const text of ["L", "Lo", "Lon", "Lond", "London"]) setSurfaceState({ where: text });
    expect((useAppStore.getState().surfaces.c1?.state as { where: string }).where).toBe("London");
    expect(saved).toHaveLength(0);
    vi.advanceTimersByTime(500);
    expect(saved).toEqual([{ id: "s1", json: '{"where":"London"}' }]);
  });

  it("is written at once when the interface sends an action", async () => {
    useAppStore.getState().setSurfaceState({ where: "Paris" });
    await useAppStore.getState().sendSurfaceAction("Chose “Go”.", { action: "go" });
    expect(saved).toEqual([{ id: "s1", json: '{"where":"Paris"}' }]);
    vi.advanceTimersByTime(1000);
    expect(saved).toHaveLength(1);
  });
});
