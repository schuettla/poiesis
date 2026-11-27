/**
 * @vitest-environment jsdom
 *
 * `PRJ-9` from the session's own ⋯ menu: add a loose chat to a project, move
 * it between projects, take it out again — never deleting it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import SessionMenu from "./SessionMenu";
import { useAppStore } from "../../lib/store";
import type { Conversation, Project } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function conv(over: Partial<Conversation> = {}): Conversation {
  return { id: "c1", title: "A chat", updatedAt: 0, messages: [], projectId: null, ...over };
}

function project(id: string, name: string): Project {
  return { id, name, rootPath: null, instructions: null, trust: "confirm", execPolicy: "ask", archived: false, updatedAt: 0 };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function setup(c: Conversation, projects: Project[]) {
  useAppStore.setState({ conversations: [c], activeConversationId: c.id, projects });
  act(() => root.render(<SessionMenu />));
  act(() => container.querySelector<HTMLButtonElement>(".session-more")!.click());
}

// The destinations open as a second menu in the document body, beside the first.
const items = (scope: ParentNode = document) => Array.from(scope.querySelectorAll<HTMLButtonElement>(".row-menu-item"));
const labels = () => items(container).map((b) => b.textContent);
const flyout = () => document.querySelector(".rail-flyout");
const click = (label: string) =>
  act(() => {
    items().find((b) => b.textContent === label)!.click();
  });

describe("the session menu's project items", () => {
  it("adds a loose chat to a project", () => {
    setup(conv(), [project("p1", "The book"), project("p2", "Job hunt")]);
    expect(labels()).toContain("Add to project…");
    expect(labels()).not.toContain("The book");
    expect(flyout(), "nothing is out until asked").toBeNull();
    click("Add to project…");
    expect(items(flyout()!).map((b) => b.textContent)).toEqual(["The book", "Job hunt"]);
    expect(container.contains(flyout()), "it is not drawn inside the clipped header").toBe(false);
    click("Job hunt");
    expect(useAppStore.getState().conversations[0].projectId).toBe("p2");
    expect(document.querySelector(".row-menu"), "both menus close").toBeNull();
  });

  it("opens on hover, closes on another line, and Escape takes back one level", () => {
    setup(conv(), [project("p1", "The book")]);
    const parent = items(container).find((b) => b.textContent === "Add to project…")!;
    act(() => void parent.dispatchEvent(new MouseEvent("pointerover", { bubbles: true })));
    act(() => void parent.dispatchEvent(new PointerEvent("pointerenter")));
    expect(flyout()).not.toBeNull();
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(flyout(), "Escape closes the flyout first").toBeNull();
    expect(container.querySelector(".row-menu"), "and leaves the menu").not.toBeNull();
    act(() => void parent.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true })));
    expect(flyout()).not.toBeNull();
    const other = items(container).find((b) => b.textContent === "Schedule this…")!;
    act(() => void other.dispatchEvent(new MouseEvent("pointerover", { bubbles: true })));
    expect(flyout(), "pointing at another line puts it away").toBeNull();
  });

  it("moves a chat between projects, offering only the others", () => {
    setup(conv({ projectId: "p1" }), [project("p1", "The book"), project("p2", "Job hunt")]);
    click("Move to project…");
    expect(labels()).not.toContain("The book");
    click("Job hunt");
    expect(useAppStore.getState().conversations[0].projectId).toBe("p2");
  });

  it("takes a chat out of its project without deleting it", () => {
    setup(conv({ projectId: "p1" }), [project("p1", "The book")]);
    expect(labels()).not.toContain("Move to project…");
    click("Remove from The book");
    const s = useAppStore.getState();
    expect(s.conversations[0].projectId).toBeNull();
    expect(s.conversations).toHaveLength(1);
  });

  it("offers nothing about projects when there are none", () => {
    setup(conv(), []);
    expect(labels()).toEqual(["Turn on Workspace mode", "Schedule this…", "Session details…", "Delete chat"]);
  });
});
