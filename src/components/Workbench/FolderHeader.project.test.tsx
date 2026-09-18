/**
 * @vitest-environment jsdom
 *
 * `PRJ-UI-5`: the project this chat belongs to, on the chat's own panel head —
 * and `PRJ-9`'s chat-side half, moving a session between projects from there.
 *
 * The case worth guarding is a project with no folder: before this, being in
 * one looked exactly like being in no project at all, because the folder offer
 * replaced the whole head.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FolderHeader from "./FolderHeader";
import { useAppStore } from "../../lib/store";
import type { Conversation, Project } from "../../lib/types";
import type { ProjectCardView } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

function conv(over: Partial<Conversation> = {}): Conversation {
  return {
    id: "c1",
    title: "c1",
    updatedAt: Date.now(),
    messages: [],
    projectId: null,
    ...over,
  };
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

function seed(conversation: Conversation, projects: Project[]) {
  useAppStore.setState({
    conversations: [conversation],
    activeConversationId: conversation.id,
    projects,
    folderError: null,
    indexState: null,
    indexProgress: null,
    indexError: null,
    indexExplained: true,
    showHidden: false,
  });
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () =>
  act(() => {
    root.render(<FolderHeader />);
  });

const text = () => container.textContent ?? "";
/** The header line is the only thing drawn at rest; everything else is in
 * the popover it opens. */
const openMenu = () =>
  act(() => {
    container.querySelector<HTMLButtonElement>(".wb-context-btn")!.click();
  });
const clickItem = (label: string) =>
  act(() => {
    Array.from(container.querySelectorAll<HTMLButtonElement>(".wb-popover button"))
      .find((b) => b.textContent === label)!
      .click();
  });

describe("the header line", () => {
  it("is one line at rest: no path, access control or code chips until asked", () => {
    seed(conv({ projectId: "p1", folderPath: "C:\\work\\book" }), [project({ rootPath: "C:\\work\\book" })]);
    render();
    expect(container.querySelector(".wb-popover")).toBeNull();
    expect(container.querySelector(".wb-segments")).toBeNull();
    expect(text()).not.toContain("C:\\work\\book");
    expect(container.querySelector(".wb-access")?.textContent).toBe("Asks first");
  });

  it("opens the popover, and Escape closes it", () => {
    seed(conv({ folderPath: "C:\\work\\loose" }), []);
    render();
    openMenu();
    expect(container.querySelector(".wb-popover")).not.toBeNull();
    expect(text()).toContain("C:\\work\\loose");
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(container.querySelector(".wb-popover")).toBeNull();
  });

  it("sets the access level from the popover", () => {
    seed(conv({ folderPath: "C:\\work\\loose", folderTrust: "confirm" }), []);
    const setFolderTrust = vi.fn();
    useAppStore.setState({ setFolderTrust });
    render();
    openMenu();
    clickItem("Read only");
    expect(setFolderTrust).toHaveBeenCalledWith("read-only");
  });
});

describe("the project on the chat (PRJ-UI-5)", () => {
  it("names the project on the header line, and the name in the popover opens its view", () => {
    seed(conv({ projectId: "p1" }), [project()]);
    render();
    expect(container.querySelector(".wb-crumb-project")?.textContent).toBe("The book");

    openMenu();
    const name = container.querySelector<HTMLButtonElement>(".wb-project-name")!;
    expect(name.textContent).toBe("The book");
    act(() => name.click());
    const s = useAppStore.getState();
    expect(s.view).toBe("project");
    expect(s.activeProjectId).toBe("p1");
  });

  it("still names the project when there is no folder, and offers one", () => {
    // A folderless project must never look like being in no project at all.
    seed(conv({ projectId: "p1" }), [project()]);
    render();
    expect(container.querySelector(".wb-crumb-project")).not.toBeNull();
    expect(container.querySelector(".wb-crumb-none")?.textContent).toBe("No folder");
    expect(container.querySelector(".wb-context-choose")).not.toBeNull();
  });

  it("puts the project before the folder when there is one", () => {
    seed(
      conv({ projectId: "p1", folderPath: "C:\\work\\book" }),
      [project({ rootPath: "C:\\work\\book" })]
    );
    render();
    const proj = container.querySelector(".wb-crumb-project")!;
    const folder = container.querySelector(".wb-folder-name")!;
    expect(proj.compareDocumentPosition(folder) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(folder.textContent).toBe("book");
  });

  it("draws nothing new for a chat with no project", () => {
    seed(conv({ folderPath: "C:\\work\\loose" }), []);
    render();
    expect(container.querySelector(".wb-crumb-project")).toBeNull();
    expect(container.querySelector(".wb-folder-name")?.textContent).toBe("loose");
  });

  it("says nothing when the project is archived out from under the chat", () => {
    seed(conv({ projectId: "p1" }), []);
    render();
    expect(container.querySelector(".wb-crumb-project")).toBeNull();
    openMenu();
    expect(container.querySelector(".wb-project-row")).toBeNull();
  });

  it("warns that detaching also leaves the project, which the folder-only copy cannot", () => {
    seed(
      conv({ projectId: "p1", folderPath: "C:\\work\\book" }),
      [project({ rootPath: "C:\\work\\book" })]
    );
    render();
    openMenu();
    clickItem("Detach folder");
    expect(text()).toContain("This chat also leaves the project");
  });
});

function cardView(over: Partial<ProjectCardView> = {}): ProjectCardView {
  return {
    card: {
      languages: ["Rust", "TypeScript"],
      manifests: ["Cargo.toml", "package.json"],
      package_managers: ["npm"],
      tasks: [
        { name: "cargo check", argv: ["cargo", "check"], cwd: "C:\\work\\app", kind: "check", source: "Cargo.toml" },
        { name: "npm test", argv: ["npm", "run", "test"], cwd: "C:\\work\\app", kind: "test", source: "package.json" },
      ],
      git: true,
      branch: "master",
      instructions_file: "AGENTS.md",
      readme: true,
    },
    policy: "ask",
    policy_is_own: false,
    allow: { tasks: [], commands: [], run_command: false },
    tasks_enabled: true,
    card_built_at: Date.now(),
    ...over,
  };
}

describe("the project header chips (COD-UI-1)", () => {
  const seedCode = (view: ProjectCardView | null, over: Partial<Conversation> = {}) => {
    seed(conv({ projectId: "p1", folderPath: "C:\\work\\app", ...over }), [project({ rootPath: "C:\\work\\app", execPolicy: "inherit" })]);
    useAppStore.setState({ projectCards: { p1: view }, expert: false });
  };
  const tasksChip = () =>
    Array.from(container.querySelectorAll<HTMLButtonElement>(".wb-chip-button")).find((b) =>
      b.textContent?.startsWith("Tasks")
    );

  it("shows languages and branch as chips, and trust only once — as the access level", () => {
    seedCode(cardView());
    render();
    openMenu();
    const chips = Array.from(container.querySelectorAll(".wb-chip")).map((c) => c.textContent);
    expect(chips).toContain("Rust + TypeScript");
    expect(chips).toContain("master");
    expect(chips).not.toContain("Ask first");
  });

  it("draws no section when nothing was detected", () => {
    seedCode(
      cardView({
        card: { languages: [], manifests: [], package_managers: [], tasks: [], git: false, branch: null, instructions_file: null, readme: false },
      })
    );
    render();
    openMenu();
    expect(container.querySelector(".wb-code")).toBeNull();
  });

  it("lists each task with its command, and says the instructions file was read", () => {
    seedCode(cardView());
    render();
    openMenu();
    act(() => tasksChip()!.click());
    expect(text()).toContain("cargo check");
    expect(text()).toContain("npm run test");
    expect(text()).toContain("AGENTS.md is read into every chat here");
    expect(container.querySelectorAll(".wb-task-allow input")).toHaveLength(2);
  });

  it("offers no per-task toggles when the folder is read only", () => {
    seedCode(cardView(), { folderTrust: "read-only" });
    render();
    openMenu();
    act(() => tasksChip()!.click());
    expect(text()).toContain("read only");
    expect(container.querySelectorAll(".wb-task-allow input")).toHaveLength(0);
  });

  it("says tasks are off in Settings rather than offering choices that do nothing", () => {
    seedCode(cardView({ tasks_enabled: false }));
    render();
    openMenu();
    act(() => tasksChip()!.click());
    expect(text()).toContain("off in Settings");
    expect(container.querySelector(".wb-tasks .wb-segments")).toBeNull();
  });

  it("shows the other-commands opt-in only in expert mode", () => {
    seedCode(cardView());
    render();
    openMenu();
    act(() => tasksChip()!.click());
    expect(text()).not.toContain("other commands");
    act(() => useAppStore.setState({ expert: true }));
    expect(text()).toContain("other commands");
  });

  it("marks the project's own policy, Default when it follows Settings", () => {
    seedCode(cardView());
    render();
    openMenu();
    act(() => tasksChip()!.click());
    const on = container.querySelector(".wb-tasks .wb-segment.on");
    expect(on?.textContent).toBe("Default");
  });
});

describe("moving a session from the chat (PRJ-9)", () => {
  it("moves it to another project", () => {
    seed(conv({ projectId: "p1" }), [project(), project({ id: "p2", name: "Job hunt" })]);
    render();
    openMenu();
    clickItem("Move this chat to…");
    clickItem("Job hunt");
    expect(useAppStore.getState().conversations[0].projectId).toBe("p2");
  });

  it("takes it out of one without deleting the chat", () => {
    seed(conv({ projectId: "p1" }), [project()]);
    render();
    openMenu();
    clickItem("Remove from project");
    const s = useAppStore.getState();
    expect(s.conversations[0].projectId).toBeNull();
    expect(s.conversations).toHaveLength(1);
  });

  it("offers no move targets when there is nowhere else to go", () => {
    seed(conv({ projectId: "p1" }), [project()]);
    render();
    openMenu();
    expect(text()).not.toContain("Move this chat to");
    expect(text()).toContain("Remove from project");
  });
});
