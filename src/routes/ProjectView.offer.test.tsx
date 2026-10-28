/**
 * @vitest-environment jsdom
 *
 * `PRJ-S1`: a project with no instructions that looks like another one offers that
 * one's instructions. Only the instructions are copied.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const similar = vi.fn();
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  similarProject: (id: string) => similar(id),
}));

import ProjectView from "./ProjectView";
import { useAppStore } from "../lib/store";
import type { Project } from "../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

function project(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Taxes 2025",
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
const saved: string[] = [];

beforeEach(() => {
  similar.mockReset();
  saved.length = 0;
  try {
    localStorage.clear();
  } catch {
    // none
  }
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    projects: [project(), project({ id: "p0", name: "Taxes 2024", instructions: "Use the bank export in /data." })],
    activeProjectId: "p1",
    conversations: [],
    activeConversationId: null,
    view: "project",
    folderError: null,
    setProjectInstructions: ((_id: string, text: string) => {
      saved.push(text);
      return Promise.resolve();
    }) as never,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = async () => {
  await act(async () => {
    root.render(<ProjectView />);
  });
};
const button = (label: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === label);

describe("the new-project offer (PRJ-S1)", () => {
  it("names the other project and why, and copies only its instructions", async () => {
    similar.mockResolvedValue({ project_id: "p0", why: 'both have "taxes" in the name' });
    await render();
    const offer = container.querySelector(".pv-offer");
    expect(offer?.textContent).toContain("Taxes 2024");
    expect(offer?.textContent).toContain('both have "taxes" in the name');
    expect(offer?.textContent).toContain("notes stay where they are");
    await act(async () => button("Copy its instructions")!.click());
    expect(saved).toEqual(["Use the bank export in /data."]);
  });

  it("is not shown when nothing looks alike, or when the project has instructions", async () => {
    similar.mockResolvedValue(null);
    await render();
    expect(container.querySelector(".pv-offer")).toBeNull();

    useAppStore.setState({ projects: [project({ instructions: "Mine." }), project({ id: "p0", name: "Taxes 2024", instructions: "Theirs." })] });
    similar.mockResolvedValue({ project_id: "p0", why: "x" });
    await render();
    expect(container.querySelector(".pv-offer")).toBeNull();
  });

  it("is not shown when the lookup fails, and Not now keeps it away", async () => {
    similar.mockRejectedValue(new Error("no backend"));
    await render();
    expect(container.querySelector(".pv-offer")).toBeNull();

    similar.mockResolvedValue({ project_id: "p0", why: "x" });
    await act(async () => root.unmount());
    root = createRoot(container);
    await render();
    expect(container.querySelector(".pv-offer")).not.toBeNull();
    await act(async () => button("Not now")!.click());
    expect(container.querySelector(".pv-offer")).toBeNull();
    await act(async () => root.unmount());
    root = createRoot(container);
    await render();
    expect(container.querySelector(".pv-offer")).toBeNull();
  });
});
