/**
 * @vitest-environment jsdom
 *
 * `PRJ-UI-4`: the project view. Four sections — name, instructions, working
 * folder, sessions — each editable where it is shown.
 *
 * The case worth guarding is the folderless one: a project with no directory
 * has to read as a normal project, not an unfinished one.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import ProjectView from "./ProjectView";
import { useAppStore } from "../lib/store";
import type { Conversation, Project } from "../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

function conv(id: string, projectId?: string): Conversation {
  return { id, title: id, updatedAt: Date.now(), messages: [], projectId: projectId ?? null };
}

function project(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "The book",
    rootPath: null,
    instructions: null,
    trust: "confirm",
    execPolicy: "ask",
    archived: false,
    updatedAt: Date.now(),
    ...over,
  };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    projects: [project()],
    activeProjectId: "p1",
    conversations: [conv("c1", "p1"), conv("c2", "p1"), conv("loose")],
    activeConversationId: "c1",
    view: "project",

    folderError: null,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () =>
  act(() => {
    root.render(<ProjectView />);
  });

const text = () => container.textContent ?? "";

/** React tracks a controlled field's last value on the DOM node itself and
 * treats a plain `el.value = …` as a no-op, so the change never reaches the
 * component. Going through the prototype's own setter is what makes the
 * synthetic `onChange` fire, which is what the user typing actually does. */
function type(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
  Object.getOwnPropertyDescriptor(proto.prototype, "value")!.set!.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("the project view (PRJ-UI-4)", () => {
  it("shows all four sections for a project with no folder", () => {
    render();
    expect(container.querySelector<HTMLInputElement>(".pv-name")?.value).toBe("The book");
    expect(container.querySelector(".pv-instructions")).not.toBeNull();
    expect(text()).toContain("Working folder");
    expect(text()).toContain("Sessions");
  });

  it("says a folderless project is normal rather than unfinished", () => {
    render();
    expect(text()).toContain("A project doesn’t need one");
    // The trust control belongs to a folder, so it is absent until there is one.
    expect(container.querySelector(".pv-trust")).toBeNull();
  });

  it("swaps the folder section's shape once a folder is attached", () => {
    act(() => {
      useAppStore.setState({ projects: [project({ rootPath: "C:\\work\\book" })] });
    });
    render();
    expect(text()).toContain("C:\\work\\book");
    expect(container.querySelector(".pv-trust")).not.toBeNull();
    expect(text()).toContain("Remove folder");
    expect(text()).not.toContain("A project doesn’t need one");
  });

  it("lists only this project's sessions", () => {
    render();
    const sessions = Array.from(container.querySelectorAll(".pv-session")).map(
      (b) => b.textContent
    );
    expect(sessions).toEqual(["c1", "c2"]);
  });

  it("commits a rename on Enter and reverts on Escape", () => {
    render();
    const name = container.querySelector<HTMLInputElement>(".pv-name")!;

    act(() => {
      type(name, "Kitchen rebuild");
      name.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      name.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(useAppStore.getState().projects[0].name).toBe("Kitchen rebuild");

    act(() => {
      type(name, "typo");
      name.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(useAppStore.getState().projects[0].name).toBe("Kitchen rebuild");
  });

  it("saves instructions on blur, not on every keystroke", () => {
    render();
    const box = container.querySelector<HTMLTextAreaElement>(".pv-instructions")!;

    act(() => {
      type(box, "Quotes are in EUR.");
    });
    expect(useAppStore.getState().projects[0].instructions ?? null).toBeNull();

    act(() => {
      box.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(useAppStore.getState().projects[0].instructions).toBe("Quotes are in EUR.");
  });

  it("opens a brand-new project with its name selected for typing", () => {
    // `PRJ-UI-1a`: `New project` lands you here, so the first thing you do is
    // say what the project is.
    act(() => {
      useAppStore.setState({ projects: [project({ name: "New project" })] });
    });
    render();
    const name = container.querySelector<HTMLInputElement>(".pv-name")!;
    expect(document.activeElement, "typing has to land here").toBe(name);
    expect(name.selectionStart).toBe(0);
    expect(name.selectionEnd).toBe("New project".length);
  });

  it("leaves a named project's name alone when the view opens", () => {
    // Focusing it every time would invite an accidental overwrite of a
    // project the user has already named.
    render();
    const name = container.querySelector<HTMLInputElement>(".pv-name")!;
    expect(name.value).toBe("The book");
    expect(document.activeElement).not.toBe(name);
  });

  it("says so plainly when the project is gone rather than showing an empty column", () => {
    act(() => {
      useAppStore.setState({ projects: [] });
    });
    render();
    expect(text()).toContain("no longer here");
  });
});
