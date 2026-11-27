/**
 * @vitest-environment jsdom
 *
 * `UI-6`: the arrow keys in the box walk the chat's earlier prompts, and an open `/` menu keeps
 * them for itself.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Composer from "./Composer";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const said = (text: string, i: number): Message => ({ id: `u${i}`, role: "user", text }) as Message;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    skills: [],
    turnModifiers: {},
    conversations: [
      {
        id: "c1",
        title: "t",
        updatedAt: 0,
        projectId: null,
        messages: [said("first question", 1), { id: "a1", role: "assistant", text: "answer" } as Message, said("second question", 2)],
      },
      { id: "c2", title: "other", updatedAt: 0, projectId: null, messages: [said("another chat", 3)] },
    ],
    activeConversationId: "c1",
    liveRuns: {},
    busyChats: {},
    changeSets: {},
    workspaceMode: false,
    toolsEnabled: true,
    view: "chat",
  } as never);
  act(() => root.render(<Composer onSend={() => {}} />));
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const input = () => container.querySelector("input[type=text]") as HTMLInputElement;
const type = (text: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
const press = (key: string, init: KeyboardEventInit = {}) =>
  act(() => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
  });

describe("earlier prompts with the arrow keys (UI-6)", () => {
  it("brings back the last prompt, then the one before, from this chat only", () => {
    press("ArrowUp");
    expect(input().value).toBe("second question");
    press("ArrowUp");
    expect(input().value).toBe("first question");
    press("ArrowUp");
    expect(input().value).toBe("first question");
  });

  it("goes newer with the down arrow, and past the newest gives the draft back", () => {
    type("my draft");
    press("ArrowUp");
    press("ArrowUp");
    expect(input().value).toBe("first question");
    press("ArrowDown");
    expect(input().value).toBe("second question");
    press("ArrowDown");
    expect(input().value).toBe("my draft");
  });

  it("takes Alt+arrow the same way", () => {
    press("ArrowUp", { altKey: true });
    expect(input().value).toBe("second question");
    press("ArrowDown", { altKey: true });
    expect(input().value).toBe("");
  });

  it("makes an edited recalled prompt the draft, and leaves the history as it was", () => {
    press("ArrowUp");
    type("second question, edited");
    // The walk ended with the edit: down has nowhere to go, up starts again from the newest.
    press("ArrowDown");
    expect(input().value).toBe("second question, edited");
    press("ArrowUp");
    expect(input().value).toBe("second question");
    press("ArrowDown");
    expect(input().value).toBe("second question, edited");
  });

  it("does nothing with no earlier prompt, and the walk starts over in another chat", () => {
    act(() => useAppStore.setState({ activeConversationId: "c2" } as never));
    press("ArrowUp");
    expect(input().value).toBe("another chat");
    act(() => useAppStore.setState({ activeConversationId: "c1" } as never));
    press("ArrowDown");
    expect(input().value).toBe("another chat");
    press("ArrowUp");
    expect(input().value).toBe("second question");
  });

  it("leaves the arrows to an open / menu", () => {
    type("/");
    expect(container.querySelector(".command-menu-row")).not.toBeNull();
    press("ArrowUp");
    expect(input().value).toBe("/");
  });
});
