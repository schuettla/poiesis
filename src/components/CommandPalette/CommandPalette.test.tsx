/**
 * @vitest-environment jsdom
 *
 * The Ctrl+K palette. Outside Tauri there is no message index, so these cover
 * what the palette decides on its own: matching, grouping, keyboard, and
 * opening the thing you picked.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import CommandPalette, { splitSnippet } from "./CommandPalette";
import { useAppStore } from "../../lib/store";
import type { Conversation, Project } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollIntoView = () => {};

function conv(id: string, title: string, projectId: string | null = null): Conversation {
  return { id, title, updatedAt: Date.now(), messages: [], projectId };
}

const project: Project = {
  id: "p1",
  name: "Harness",
  rootPath: "C:\\work\\harness",
  trust: "confirm",
  execPolicy: "ask",
  archived: false,
  updatedAt: Date.now(),
};

let container: HTMLDivElement;
let root: Root;
let openSession: Mock<(id: string) => Promise<void>>;
let openProjectView: Mock<(projectId: string) => void>;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  openSession = vi.fn<(id: string) => Promise<void>>().mockResolvedValue(undefined);
  openProjectView = vi.fn<(projectId: string) => void>();
  useAppStore.setState({
    conversations: [conv("c1", "Cache invalidation bug", "p1"), conv("c2", "Weekend recipes")],
    projects: [project],
    allArtifacts: [],
    paletteOpen: false,
    openSession,
    openProjectView,
  });
  act(() => root.render(<CommandPalette />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const press = (key: string, init: KeyboardEventInit = {}) =>
  act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  });

const input = () => container.querySelector<HTMLInputElement>(".palette-input")!;

function type(text: string) {
  const el = input();
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

const keyInInput = (key: string) =>
  act(() => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });

const groupLabels = () =>
  Array.from(container.querySelectorAll(".palette-group-label")).map((l) => l.textContent);
const selected = () =>
  container.querySelector('[role="option"][aria-selected="true"] .palette-label')?.textContent;

describe("the command palette", () => {
  it("opens and closes on Ctrl+K", () => {
    expect(input()).toBeNull();
    press("k", { ctrlKey: true });
    expect(input()).not.toBeNull();
    expect(document.activeElement).toBe(input());
    press("k", { ctrlKey: true });
    expect(container.querySelector(".palette")).toBeNull();
  });

  it("offers recent chats, projects and commands before anything is typed", () => {
    press("k", { ctrlKey: true });
    expect(groupLabels()).toEqual(["Recent", "Projects", "Commands"]);
  });

  it("finds projects too, and names the project a chat lives in", () => {
    press("k", { ctrlKey: true });
    type("harness");
    expect(groupLabels()).toContain("Projects");
    type("cache");
    const chat = container.querySelector('[role="option"]')!;
    expect(chat.querySelector(".palette-label")?.textContent).toBe("Cache invalidation bug");
    expect(chat.querySelector(".palette-meta")?.textContent).toBe("Harness");
  });

  it("moves with the arrows and opens the selection on Enter", () => {
    press("k", { ctrlKey: true });
    type("e");
    const first = selected();
    keyInInput("ArrowDown");
    expect(selected()).not.toBe(first);
    keyInInput("ArrowUp");
    expect(selected()).toBe(first);

    type("recipes");
    keyInInput("Enter");
    expect(openSession).toHaveBeenCalledWith("c2");
    expect(useAppStore.getState().paletteOpen).toBe(false);
  });

  it("says so when nothing matches", () => {
    press("k", { ctrlKey: true });
    type("zzqx");
    expect(container.querySelector(".palette-empty")?.textContent).toBe("Nothing matches “zzqx”.");
  });

  it("closes on Escape", () => {
    press("k", { ctrlKey: true });
    keyInInput("Escape");
    expect(useAppStore.getState().paletteOpen).toBe(false);
  });
});

describe("splitSnippet", () => {
  it("splits on the database's match fences and never produces markup", () => {
    expect(splitSnippet("…the \u0002cache\u0003 was\n\nstale")).toEqual([
      { text: "…the ", hit: false },
      { text: "cache", hit: true },
      { text: " was stale", hit: false },
    ]);
    expect(splitSnippet("<b>\u0002x\u0003</b>")[0]).toEqual({ text: "<b>", hit: false });
  });
});
