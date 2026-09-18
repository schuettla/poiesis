/**
 * @vitest-environment jsdom
 *
 * The row anatomy: a title, a reading in the trailing slot, and every action
 * behind one ⋯ — nothing else holding width in the row's flow at rest.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Rail from "./Rail";
import { useAppStore } from "../../lib/store";
import type { Conversation } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const HOUR = 3_600_000;

function conv(id: string, updatedAt: number): Conversation {
  return { id, title: id, updatedAt, messages: [], projectId: null };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const now = Date.now();
  useAppStore.setState({
    conversations: [conv("fresh", now - 5 * 60_000), conv("ancient", now - 400 * 24 * HOUR)],
    projects: [],
    expandedProjects: [],
    activeConversationId: "fresh",
    view: "chat",
    railCollapsed: false,
    changeProposals: [],
    reflectingIds: [],
    digestedIds: [],
    runningJob: null,
    paletteOpen: false,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () =>
  act(() => {
    root.render(<Rail />);
  });

const row = (title: string) =>
  Array.from(container.querySelectorAll<HTMLElement>(".chat-row")).find(
    (li) => li.querySelector(".chat-title")?.textContent === title
  )!;

describe("chat rows", () => {
  it("show when they were last touched, in the slot the menu takes on hover", () => {
    render();
    expect(row("fresh").querySelector(".row-slot .row-stamp")?.textContent).toBe("5m");
    expect(row("fresh").querySelector(".row-slot .chat-more")).not.toBeNull();
  });

  it("hold no reflect button in the row; reflecting is a menu item", () => {
    const reflect = vi.fn().mockResolvedValue({ learned: 0, proposed: 0 });
    act(() => useAppStore.setState({ reflectConversation: reflect }));
    render();
    const r = row("fresh");
    expect(r.querySelectorAll("button").length, "only the ⋯").toBe(1);

    act(() => r.querySelector<HTMLButtonElement>(".chat-more")!.click());
    const item = Array.from(r.querySelectorAll<HTMLButtonElement>(".row-menu-item")).find(
      (b) => b.textContent === "Reflect on this chat"
    );
    act(() => item!.click());
    expect(reflect).toHaveBeenCalledWith("fresh");
  });

  it("drop the reflect offer once the chat has taught me something, and show the mark instead", () => {
    act(() => useAppStore.setState({ digestedIds: ["fresh"] }));
    render();
    const r = row("fresh");
    expect(r.querySelector(".chat-digest")?.getAttribute("aria-label")).toMatch(/learned/);
    act(() => r.querySelector<HTMLButtonElement>(".chat-more")!.click());
    const labels = Array.from(r.querySelectorAll(".row-menu-item")).map((b) => b.textContent);
    expect(labels).toEqual(["Schedule this…", "Delete chat"]);
  });

  it("mark the current chat for assistive tech, not only by color", () => {
    render();
    expect(row("fresh").getAttribute("aria-current")).toBe("page");
    expect(row("ancient").getAttribute("aria-current")).toBeNull();
  });
});

describe("the date groups", () => {
  it("fall into calendar buckets instead of one endless Earlier", () => {
    render();
    const labels = Array.from(container.querySelectorAll(".rail-group .rail-label")).map(
      (l) => l.textContent
    );
    expect(labels).toEqual(["Today", "Older"]);
  });
});

describe("the Search row", () => {
  it("opens the palette", () => {
    render();
    act(() => container.querySelector<HTMLButtonElement>(".search-btn")!.click());
    expect(useAppStore.getState().paletteOpen).toBe(true);
  });
});
