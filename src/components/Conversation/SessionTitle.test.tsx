/**
 * @vitest-environment jsdom
 *
 * The chat's name is edited where the header shows it, and its menu is the chevron beside it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import SessionTitle from "./SessionTitle";
import TabStrip from "../TopBar/TabStrip";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [{ id: "c1", title: "Old name", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    itemTabs: [],
    activeItemId: null,
    projects: [],
    engines: [],
    view: "chat",
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const title = () => useAppStore.getState().conversations[0].title;
const field = () => container.querySelector<HTMLInputElement>("input.session-title-input");

/** React tracks the value itself, so a plain assignment is not seen as a change. */
function type(el: HTMLInputElement, value: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const key = (el: Element, k: string) =>
  act(() => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));

describe("renaming the chat in the header", () => {
  const mount = () => act(() => root.render(<SessionTitle className="x" activate="click" fallback="New chat" />));

  it("opens on a click, and Enter keeps the new name", () => {
    mount();
    act(() => container.querySelector<HTMLButtonElement>("button")!.click());
    expect(field()!.value).toBe("Old name");
    type(field()!, "  Quarterly report ");
    key(field()!, "Enter");
    expect(title()).toBe("Quarterly report");
    expect(field()).toBeNull();
  });

  it("drops the edit on Escape", () => {
    mount();
    act(() => container.querySelector<HTMLButtonElement>("button")!.click());
    type(field()!, "Nope");
    key(field()!, "Escape");
    expect(title()).toBe("Old name");
    expect(field()).toBeNull();
  });

  it("keeps the old name when the field is emptied", () => {
    mount();
    act(() => container.querySelector<HTMLButtonElement>("button")!.click());
    type(field()!, "   ");
    key(field()!, "Enter");
    expect(title()).toBe("Old name");
  });
});

describe("the session tab", () => {
  const mount = () => {
    useAppStore.setState({
      itemTabs: [{ kind: "file", id: "C:\\work\\a.ts", conversationId: "c1" }],
      activeItemId: "file:C:\\work\\a.ts",
    } as never);
    act(() => root.render(<TabStrip />));
  };

  it("renames on a double click, not a single one", () => {
    mount();
    const label = container.querySelector<HTMLElement>(".ts-session .ts-tab-label")!;
    act(() => label.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(field()).toBeNull();
    act(() => label.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
    expect(field()).not.toBeNull();
    // Space and arrows belong to the field, not to the strip's own key handling.
    const space = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    act(() => void field()!.dispatchEvent(space));
    expect(space.defaultPrevented).toBe(false);
  });

  it("carries the chat's menu as a chevron, which does not switch tabs", () => {
    mount();
    const more = container.querySelector<HTMLButtonElement>(".ts-session .session-more")!;
    act(() => more.click());
    expect(container.querySelector(".ts-session .row-menu")).not.toBeNull();
    expect(container.querySelector(".ts-session.active")).toBeNull();
    // The header drags the window from anywhere not marked otherwise; the backdrop must be marked.
    const backdrop = container.querySelector<HTMLElement>(".row-menu-backdrop")!;
    expect(backdrop.hasAttribute("data-no-drag")).toBe(true);
    act(() => backdrop.click());
    expect(container.querySelector(".row-menu")).toBeNull();
    act(() => more.click());
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(container.querySelector(".row-menu")).toBeNull();
  });
});
