/**
 * @vitest-environment jsdom
 *
 * `REM-UI-2`, `REM-UI-3`: what the gateway tells the window. The mark's `remoteReachable` follows
 * `poiesis-remote`; a turn a phone started is shown live in the open chat through the same fold
 * as a typed one; a question answered on the phone leaves no card on the desktop.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, DbMessage } from "./api";

const bus = vi.hoisted(() => ({
  handlers: new Map<string, (payload: unknown) => void>(),
  rows: [] as unknown[],
  chats: [] as unknown[],
  status: { enabled: false, connected: false, senders: [] as unknown[] },
}));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  onAppEvent: (name: string, handler: (payload: unknown) => void) => void bus.handlers.set(name, handler),
  remoteStatus: async () => bus.status,
  listMessages: async () => bus.rows,
  listConversations: async () => bus.chats,
  refreshChanges: async () => {},
}));

import { listenForRemote, useAppStore } from "./store";

const set = (patch: object) => useAppStore.setState(patch as never);
const get = () => useAppStore.getState();
const emit = (name: string, payload: unknown) => bus.handlers.get(name)!(payload);
const settle = () => new Promise((r) => setTimeout(r, 0));

const row = (id: string, role: "user" | "assistant", content: string): DbMessage => ({
  id,
  conversation_id: "c1",
  role,
  content,
  model_name: null,
  model_provenance: null,
  steps_json: null,
  stop_reason: null,
  plan_json: null,
  created_at: 1,
});

const model = { id: "m1", name: "Tiny", provenance: "local", available: true };
const turn = (event: AgentEvent) => emit("poiesis-remote-turn", { conversation_id: "c1", event });

beforeEach(() => {
  bus.handlers.clear();
  bus.rows = [];
  bus.chats = [];
  bus.status = { enabled: false, connected: false, senders: [] };
  set({
    conversations: [{ id: "c1", title: "Phone", updatedAt: 0, messages: [] }],
    models: [model],
    selectedModelId: "m1",
    busyChats: {},
    pendingPermissions: [],
    remoteReachable: false,
  });
  listenForRemote(useAppStore.setState as never, get);
});

describe("the mark's ring", () => {
  it("follows the gateway: on and connected only", async () => {
    emit("poiesis-remote", { enabled: true, connected: false });
    expect(get().remoteReachable).toBe(false);
    emit("poiesis-remote", { enabled: true, connected: true });
    expect(get().remoteReachable).toBe(true);
    emit("poiesis-remote", { enabled: false, connected: true });
    expect(get().remoteReachable).toBe(false);
  });

  it("starts from what the gateway already says", async () => {
    bus.status = { enabled: true, connected: true, senders: [] };
    listenForRemote(useAppStore.setState as never, get);
    await settle();
    expect(get().remoteReachable).toBe(true);
  });
});

describe("a turn a phone started", () => {
  it("is shown live in the open chat, and the desktop is free again at the end", async () => {
    bus.rows = [row("u1", "user", "What is on today?"), row("a1", "assistant", "")];
    turn({ type: "turn_saved", user_message_id: "u1", assistant_message_id: "a1" });
    turn({ type: "token", text: "Two " });
    await settle();
    // The saved rows came in, and the answer is streaming into the second.
    const msgs = () => get().conversations[0].messages;
    expect(msgs().map((m) => m.id)).toEqual(["u1", "a1"]);
    expect(msgs()[0].text).toBe("What is on today?");
    expect(get().busyChats).toHaveProperty("c1");
    turn({ type: "token", text: "meetings." });
    expect(msgs()[1].text).toBe("Two meetings.");
    expect(msgs()[1].streaming).toBe(true);
    turn({ type: "done" });
    turn({ type: "turn_finalized", message_id: "a1", steps_json: null });
    await settle();
    expect(msgs()[1].streaming).toBe(false);
    expect(get().busyChats).toEqual({});
  });

  it("shows a question as a card here too, and drops it when the phone answered", async () => {
    bus.rows = [row("u1", "user", "Send it"), row("a1", "assistant", "")];
    turn({ type: "turn_saved", user_message_id: "u1", assistant_message_id: "a1" });
    await settle();
    turn({ type: "permission", request: { id: "perm_1", summary: "Send the mail?" } } as AgentEvent);
    expect(get().pendingPermissions.map((p) => p.id)).toEqual(["perm_1"]);
    turn({ type: "permission_gone", id: "perm_1" });
    expect(get().pendingPermissions).toEqual([]);
    turn({ type: "turn_finalized", message_id: "a1", steps_json: null });
    await settle();
  });

  it("brings a new phone chat and the title from its first message into the rail, and leaves desktop chats alone", async () => {
    const chat = (id: string, title: string, sender: string | null) => ({ id, title, updated_at: 5, remote_sender: sender });
    set({ conversations: [{ id: "c1", title: "Phone", updatedAt: 0, messages: [] }, { id: "d1", title: "Mine", updatedAt: 0, messages: [] }] });
    bus.chats = [chat("c1", "What is on today?", "My phone"), chat("c2", "Book a table", "My phone"), chat("d1", "Renamed elsewhere", null)];
    turn({ type: "turn_saved", user_message_id: "u1", assistant_message_id: "a1" });
    await settle();
    const titles = Object.fromEntries(get().conversations.map((c) => [c.id, c.title]));
    expect(titles).toEqual({ c1: "What is on today?", c2: "Book a table", d1: "Mine" });
    turn({ type: "turn_finalized", message_id: "a1", steps_json: null });
    await settle();
  });

  it("is not drawn for a chat that is not loaded, or while a typed turn runs", async () => {
    set({ conversations: [] });
    turn({ type: "turn_saved", user_message_id: "u1", assistant_message_id: "a1" });
    await settle();
    expect(get().busyChats).toEqual({});

    set({ conversations: [{ id: "c1", title: "Phone", updatedAt: 0, messages: [] }], busyChats: { c1: "other" } });
    turn({ type: "turn_saved", user_message_id: "u2", assistant_message_id: "a2" });
    turn({ type: "token", text: "ignored" });
    await settle();
    expect(get().conversations[0].messages).toEqual([]);
  });
});
