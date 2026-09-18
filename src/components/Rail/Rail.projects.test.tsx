/**
 * @vitest-environment jsdom
 *
 * `PRJ-UI-1`: the Projects group in the Rail. The thing worth testing is that
 * it is a *group* and not a second navigation model — the loose chats keep
 * their date groups underneath, and no conversation is listed twice.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Rail from "./Rail";
import { useAppStore } from "../../lib/store";
import type { Conversation, Project } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

function conv(id: string, projectId?: string): Conversation {
  return { id, title: id, updatedAt: Date.now(), messages: [], projectId: projectId ?? null };
}

function project(id: string): Project {
  return {
    id,
    name: id,
    rootPath: `C:\\work\\${id}`,
    trust: "confirm",
    execPolicy: "ask",
    archived: false,
    updatedAt: Date.now(),
  };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [conv("in-alpha", "alpha"), conv("also-alpha", "alpha"), conv("loose")],
    projects: [project("alpha")],
    expandedProjects: [],
    activeConversationId: "loose",

    view: "chat",
    railCollapsed: false,
    changeProposals: [],
    reflectingIds: [],
    digestedIds: [],
    runningJob: null,
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

const titles = (selector: string) =>
  Array.from(container.querySelectorAll<HTMLElement>(selector)).map(
    (el) => el.textContent?.trim() ?? ""
  );

describe("the Projects group (PRJ-UI-1)", () => {
  it("lists projects above the chats, and keeps their sessions out of the date groups", () => {
    render();

    expect(titles(".project-row .chat-title")).toEqual(["alpha"]);
    // Only the loose chat is left below. A session listed both under its
    // project and under Today would read as two conversations.
    const rows = titles(".chat-list li:not(.project-row-wrap) .chat-title");
    expect(rows).toEqual(["loose"]);
  });

  it("shows how many sessions a project holds without expanding it", () => {
    render();
    expect(container.querySelector(".project-count")?.textContent).toBe("2");
  });

  it("lists the project's sessions in place once it is expanded", () => {
    act(() => {
      useAppStore.setState({ expandedProjects: ["alpha"] });
    });
    render();
    expect(titles(".project-sessions .chat-title")).toEqual(["in-alpha", "also-alpha"]);
  });

  it("draws no group at all with no projects, so a user who never attached a folder sees today's Rail", () => {
    act(() => {
      useAppStore.setState({ projects: [], conversations: [conv("loose")] });
    });
    render();
    expect(container.querySelector(".project-list")).toBeNull();
    expect(titles(".rail-label")).not.toContain("Projects");
  });

  it("offers Projects beside New chat, with a quick-create extension", () => {
    render();
    expect(container.querySelector(".rail-top-btn.projects-btn")).not.toBeNull();
    expect(container.querySelector(".rail-top-btn-add")).not.toBeNull();
  });
});

describe("the engine readout sits beside Settings", () => {
  // It used to hold a reserved 175px segment of the header, which cost the tab
  // strip that width permanently for something idle and silent nearly all the
  // time. It belongs next to the cog: that is the row you press when the
  // engine is what you want to do something about.
  //
  // `inTauri()` is false here, so the component renders null — what these
  // assert is that the *slot* is in the Rail's footer and nowhere else.
  it("renders inside the Settings row rather than the header", () => {
    render();
    const footer = container.querySelector(".rail-nav-footer li")!;
    expect(footer).not.toBeNull();
    // The label is still there; the readout is its neighbour, not its
    // replacement.
    expect(footer.querySelector(".nav-label")?.textContent).toBe("Settings");
    expect(container.querySelector(".topbar-right")).toBeNull();
  });

  it("drops to the dot alone when the rail is collapsed to icons", () => {
    // Nothing is lost: the dot's own title and aria-label carry the whole
    // sentence, which is why the words can go rather than be squeezed.
    act(() => {
      useAppStore.setState({ railCollapsed: true });
    });
    render();
    expect(container.querySelector(".rail")?.className).toContain("collapsed");
  });
});

describe("the rail keeps listing conversations inside a route", () => {
  it("does not swap itself for the settings sections", () => {
    // `SHL-16` did exactly that, and it was withdrawn: those sections are the
    // inside of Settings and navigate from inside Settings. Taking the chats
    // and projects away to show them made them read as another top-level
    // place to be.
    act(() => {
      useAppStore.setState({ view: "settings", railCollapsed: false });
    });
    render();

    expect(container.querySelector(".rail-sections")).toBeNull();
    expect(container.querySelector(".search-btn"), "search went with them").not.toBeNull();
    expect(titles(".project-row .chat-title")).toEqual(["alpha"]);
    expect(titles(".chat-list li:not(.project-row-wrap) .chat-title")).toEqual(["loose"]);
  });
});
