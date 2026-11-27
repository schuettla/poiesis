/**
 * `HRN-UI-5`: Try again and Edit on one of your messages: what is asked first, what
 * is removed from the chat and what is sent in its place.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Message } from "./types";

const changesSince = vi.fn();
const rewindConversation = vi.fn();

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  conversationLeader: () => Promise.resolve(null),
  changesSince: (...a: unknown[]) => changesSince(...a),
  rewindConversation: (...a: unknown[]) => rewindConversation(...a),
}));

import { useAppStore } from "./store";

const sendMessage = vi.fn();
const turn = (id: string, role: Message["role"], text: string, over: Partial<Message> = {}): Message => ({
  id,
  role,
  text,
  createdAt: 1,
  ...over,
});
const get = () => useAppStore.getState();
const left = () => get().conversations[0].messages.map((m) => m.id);
const chat = (messages: Message[]) =>
  useAppStore.setState({ conversations: [{ id: "c1", title: "t", updatedAt: 0, messages }] });

beforeEach(() => {
  changesSince.mockReset().mockResolvedValue(0);
  rewindConversation.mockReset().mockResolvedValue({ prompt: "", files_undone: 0, files_failed: [] });
  sendMessage.mockReset().mockResolvedValue(undefined);
  const model = get().models[0];
  useAppStore.setState({
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    busyChats: {},
    rewindRequest: null,
    rewindText: null,
    rewindToast: null,
    sendMessage,
    setActiveConversation: vi.fn().mockResolvedValue(undefined),
  });
  chat([turn("m1", "user", "one"), turn("m2", "assistant", "1"), turn("m3", "user", "two"), turn("m4", "assistant", "2")]);
});

describe("Try again on your message", () => {
  it("replaces the last exchange without asking", async () => {
    await get().retryTurn("m3");
    expect(rewindConversation).toHaveBeenCalledWith("c1", "m3", false, true);
    expect(left()).toEqual(["m1", "m2"]);
    expect(sendMessage).toHaveBeenCalledWith("two", [], { again: true });
  });

  it("sends a click in the interface again as a click", async () => {
    const click = "Chose red\n\n```poiesis-action\n{}\n```";
    chat([turn("m1", "user", click), turn("m2", "assistant", "ok")]);
    await get().retryTurn("m1");
    expect(sendMessage).toHaveBeenCalledWith(click, [], { again: true, action: "Chose red" });
  });

  it("asks before later messages of yours go with it", async () => {
    await get().retryTurn("m1");
    expect(get().rewindRequest).toBe("m1");
    expect(get().rewindText).toBeNull();
    expect(rewindConversation).not.toHaveBeenCalled();
  });
});

describe("an edited message", () => {
  it("removes what followed and sends the new words", async () => {
    await get().retryTurn("m1", "uno");
    expect(rewindConversation).toHaveBeenCalledWith("c1", "m1", false, true);
    expect(left()).toEqual([]);
    expect(sendMessage).toHaveBeenCalledWith("uno", [], { again: true });
  });

  it("asks about changed files and carries the new words to the dialog", async () => {
    changesSince.mockResolvedValue(2);
    await get().retryTurn("m3", "dos");
    expect(get().rewindRequest).toBe("m3");
    expect(get().rewindText).toBe("dos");
    expect(rewindConversation).not.toHaveBeenCalled();
  });

  it("keeps the skill and what was attached", async () => {
    const file: NonNullable<Message["attachments"]>[number] = { id: "f", kind: "pdf", name: "a.pdf", path: "C:/a.pdf" };
    chat([turn("m1", "user", "old", { command: { name: "review", args: "old" }, attachments: [file] })]);
    await get().retryTurn("m1", "new");
    expect(sendMessage).toHaveBeenCalledWith("new", [file], { again: true, skill: "review", skillArgs: "new" });
  });

  it("says Run /name when the words after a skill are emptied", async () => {
    chat([turn("m1", "user", "Run /review", { command: { name: "review", args: "" } })]);
    await get().retryTurn("m1", "");
    expect(sendMessage).toHaveBeenCalledWith("Run /review", [], { again: true, skill: "review", skillArgs: undefined });
  });

  it("leaves everything when the removal is refused", async () => {
    rewindConversation.mockRejectedValue("I'm still working in this chat. Stop me first.");
    await get().retryTurn("m3", "dos");
    expect(left()).toEqual(["m1", "m2", "m3", "m4"]);
    expect(sendMessage).not.toHaveBeenCalled();
    expect(get().rewindToast).toContain("Stop me first");
  });
});
