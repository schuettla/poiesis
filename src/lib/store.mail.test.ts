/**
 * `USE-UI-4`: "Ask about this" on an Inbox row starts a new chat, with tools on so
 * the mail can be read, and asks for a summary of that one message.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "./store";

const sendMessage = vi.fn(async () => {});
const newConversation = vi.fn(async () => {
  useAppStore.setState({ activeConversationId: "c1" });
});

beforeEach(() => {
  sendMessage.mockClear();
  newConversation.mockClear();
  useAppStore.setState({ sendMessage, newConversation, toolsEnabled: false, view: "mail", activeConversationId: null });
});

describe("askAboutMail", () => {
  it("opens a new chat with tools on and asks for the summary", async () => {
    await useAppStore.getState().askAboutMail("Ana Ruiz", "Contract draft for Thursday");
    expect(newConversation).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().toolsEnabled).toBe(true);
    expect(useAppStore.getState().view).toBe("chat");
    expect(sendMessage).toHaveBeenCalledWith("Summarise the mail from Ana Ruiz, 'Contract draft for Thursday'");
  });

  it("names the account when the person has several", async () => {
    await useAppStore.getState().askAboutMail("GitHub", "CI failed", "Work");
    expect(sendMessage).toHaveBeenCalledWith("Summarise the mail from GitHub in my Work account, 'CI failed'");
  });

  it("sends nothing when no chat could be started", async () => {
    newConversation.mockImplementationOnce(async () => {});
    await useAppStore.getState().askAboutMail("Ana Ruiz", "Hi");
    expect(sendMessage).not.toHaveBeenCalled();
    expect(useAppStore.getState().view).toBe("mail");
  });
});
