// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";
import UserTurn from "./UserTurn";

/**
 * `HRN-UI-5`: Copy, Try again and Edit on your own message.
 */
(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const retryTurn = vi.fn().mockResolvedValue(undefined);
const initial = useAppStore.getState();

const mine = (over: Partial<Message> = {}): Message => ({
  id: "m1",
  role: "user",
  text: "Which planet is the largest?",
  createdAt: 1,
  ...over,
});

const show = (m: Message, focus = false) => act(() => root.render(<UserTurn message={m} focus={focus} />));
const labels = () => [...container.querySelectorAll(".turn-actions button")].map((b) => b.getAttribute("aria-label"));
const button = (name: string) =>
  [...container.querySelectorAll("button")].find((b) => (b.getAttribute("aria-label") ?? b.textContent) === name)!;
const press = async (name: string) => act(async () => button(name).click());
const box = () => container.querySelector("textarea")!;
const type = (value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(box(), value);
    box().dispatchEvent(new Event("input", { bubbles: true }));
  });
const key = async (k: string, shiftKey = false) =>
  act(async () => {
    box().dispatchEvent(new KeyboardEvent("keydown", { key: k, shiftKey, bubbles: true }));
  });

beforeEach(() => {
  retryTurn.mockClear();
  useAppStore.setState({ retryTurn, activeConversationId: "c1", busyChats: {} });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState(initial, true);
});

describe("your own message", () => {
  it("offers Copy, Try again and Edit", () => {
    show(mine());
    expect(labels()).toEqual(["Copy", "Try again", "Edit"]);
  });

  it("only lets you copy while the chat works, and a turn that is not saved yet", () => {
    useAppStore.setState({ busyChats: { c1: "other" } });
    show(mine());
    expect(labels()).toEqual(["Copy"]);
    useAppStore.setState({ busyChats: {} });
    show(mine({ id: "u-123" }));
    expect(labels()).toEqual(["Copy"]);
  });

  it("does not start over from a message that went into a working run", () => {
    show(mine({ midRun: "delivered" }));
    expect(labels()).toEqual(["Copy"]);
  });

  it("shows nothing of it in focus", () => {
    show(mine(), true);
    expect(labels()).toEqual([]);
  });

  it("copies a skill command as you typed it", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    show(mine({ text: "the parser", command: { name: "review", args: "the parser" } }));
    await press("Copy");
    expect(writeText).toHaveBeenCalledWith("/review the parser");
  });

  it("tries the message again", async () => {
    show(mine());
    await press("Try again");
    expect(retryTurn).toHaveBeenCalledWith("m1");
  });

  it("opens the message for changes and sends the new words with Enter", async () => {
    show(mine());
    await press("Edit");
    expect(box().value).toBe("Which planet is the largest?");
    expect(container.textContent).toContain("Everything after this message is removed");
    type("Which moon is the largest?");
    await key("Enter", true);
    expect(retryTurn).not.toHaveBeenCalled();
    await key("Enter");
    expect(retryTurn).toHaveBeenCalledWith("m1", "Which moon is the largest?");
  });

  it("leaves the message as it was on Escape", async () => {
    show(mine());
    await press("Edit");
    type("Something else");
    await key("Escape");
    expect(container.querySelector("textarea")).toBeNull();
    expect(container.textContent).toContain("Which planet is the largest?");
    expect(retryTurn).not.toHaveBeenCalled();
  });

  it("does not send an empty message", async () => {
    show(mine());
    await press("Edit");
    type("   ");
    expect(button("Send").disabled).toBe(true);
    await key("Enter");
    expect(retryTurn).not.toHaveBeenCalled();
  });

  it("edits what followed a skill's name", async () => {
    show(mine({ text: "the parser", command: { name: "review", args: "the parser" } }));
    await press("Edit");
    expect(box().value).toBe("the parser");
    expect(container.textContent).toContain("/review");
  });

  it("has nothing to edit on a click in the interface", () => {
    show(mine({ text: "Chose red\n```poiesis-action\n{}\n```" }));
    expect(labels()).toEqual(["Copy", "Try again"]);
  });
});
