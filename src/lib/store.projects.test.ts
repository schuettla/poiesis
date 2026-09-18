/**
 * `PRJ-3`/`PRJ-UI-2`: what the project entity changes on the frontend side —
 * which project a new session lands in, and that the strip stays global.
 *
 * Runs with `api.inTauri()` false, so nothing is persisted and only the local
 * state transitions are under test. The database half (the migration, the
 * folder resolver, create-or-join) is tested in `db/mod.rs`, where it lives.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { composeSystemPrompt, useAppStore } from "./store";
import type { Conversation, Project } from "./types";

function conv(id: string, projectId?: string): Conversation {
  return { id, title: id, updatedAt: 0, messages: [], projectId: projectId ?? null };
}

function project(id: string, tabsJson?: string): Project {
  return {
    id,
    name: id,
    rootPath: `C:\\work\\${id}`,
    trust: "confirm",
    execPolicy: "ask",
    archived: false,
    updatedAt: 0,
    tabsJson: tabsJson ?? null,
  };
}

/** `PRJ-1a`: a project that is not about a directory — a book, a job, a
 * person. The kind this whole change exists for. */
function folderless(id: string, instructions?: string): Project {
  return { ...project(id), rootPath: null, instructions: instructions ?? null };
}

beforeEach(() => {
  useAppStore.setState({
    conversations: [conv("p1a", "p1"), conv("p1b", "p1"), conv("p2a", "p2"), conv("loose")],
    projects: [project("p1", JSON.stringify({ dockView: "artifacts" })), project("p2")],
    activeConversationId: "p1a",
    itemTabs: [],
    activeItemId: null,
    selected: null,
    expandedProjects: [],
    view: "chat",
    artifacts: {},
  });
});

describe("a new session lands in the project you are in (PRJ-UI-2)", () => {
  it("carries the project and its folder onto the new chat", async () => {
    useAppStore.setState((s) => ({
      conversations: s.conversations.map((c) =>
        c.id === "p1a" ? { ...c, folderPath: "C:\\work\\p1" } : c
      ),
    }));

    await useAppStore.getState().newConversation();

    const s = useAppStore.getState();
    const created = s.conversations.find((c) => c.id === s.activeConversationId)!;
    expect(created.projectId).toBe("p1");
    expect(created.folderPath).toBe("C:\\work\\p1");
  });

  it("leaves a chat outside any project loose", async () => {
    await useAppStore.getState().setActiveConversation("loose");
    await useAppStore.getState().newConversation();

    const s = useAppStore.getState();
    const created = s.conversations.find((c) => c.id === s.activeConversationId)!;
    expect(created.projectId).toBeNull();
    // And no folder is inherited either — a loose chat pointing at the last
    // project's folder would be the folder attached without anyone asking.
    expect(created.folderPath ?? null).toBeNull();
  });
});

describe("the strip is one global set, whichever project you are in", () => {
  it("keeps an item tab open across the switch, unfocused", async () => {
    useAppStore.getState().openItem({ kind: "artifact", id: "a1" });
    await useAppStore.getState().setActiveConversation("p2a");
    const s = useAppStore.getState();
    expect(s.itemTabs).toEqual([{ kind: "artifact", id: "a1", conversationId: "p1a" }]);
    expect(s.activeItemId).toBeNull();
  });

  it("pressing an item from another chat makes that chat live and shows the item", () => {
    useAppStore.getState().openItem({ kind: "artifact", id: "a1" });
    useAppStore.getState().setActiveConversation("p2a");
    useAppStore.getState().openItem({ kind: "artifact", id: "a1", conversationId: "p1a" });
    const s = useAppStore.getState();
    expect(s.activeConversationId).toBe("p1a");
    expect(s.activeItemId).toBe("artifact:a1");
  });

  it("closing an item never switches chats to reach a neighbour", () => {
    useAppStore.getState().openItem({ kind: "artifact", id: "a1" });
    useAppStore.getState().setActiveConversation("p2a");
    useAppStore.getState().openItem({ kind: "artifact", id: "a2" });
    useAppStore.getState().closeItem("artifact:a2");
    const s = useAppStore.getState();
    expect(s.activeConversationId).toBe("p2a");
    expect(s.activeItemId).toBeNull();
    expect(s.itemTabs.map((t) => t.id)).toEqual(["a1"]);
  });

  it("drops a deleted chat's item tabs with it", async () => {
    useAppStore.getState().openItem({ kind: "artifact", id: "a1" });
    await useAppStore.getState().setActiveConversation("p2a");
    await useAppStore.getState().deleteConversation("p1a");
    expect(useAppStore.getState().itemTabs).toEqual([]);
  });

});

describe("trust is granted for the folder, not the chat (PRJ-4)", () => {
  it("reaches every session in the project, and the project row itself", async () => {
    await useAppStore.getState().setFolderTrust("read-only");

    const s = useAppStore.getState();
    for (const id of ["p1a", "p1b"]) {
      expect(s.conversations.find((c) => c.id === id)!.folderTrust).toBe("read-only");
    }
    expect(s.projects.find((p) => p.id === "p1")!.trust).toBe("read-only");
    // And stops at the project's edge.
    expect(s.conversations.find((c) => c.id === "p2a")!.folderTrust).toBeUndefined();
  });
});

describe("a project does not need a folder (PRJ-1a)", () => {
  it("makes a folderless project and opens its view, with no picker in the way", async () => {
    // `PRJ-UI-1a`: opening a file dialog first would say a project *is* a
    // folder, which is the thing this change exists to stop saying.
    await useAppStore.getState().newProject();

    const s = useAppStore.getState();
    const created = s.projects.find((p) => p.id === s.activeProjectId)!;
    expect(created.rootPath).toBeNull();
    expect(created.name).toBe("New project");
    expect(s.view).toBe("project");
  });

  it("opens a project's view rather than guessing at one of its chats", () => {
    useAppStore.getState().openProjectView("p2");
    const s = useAppStore.getState();
    expect(s.activeProjectId).toBe("p2");
    expect(s.view).toBe("project");
  });

  it("keeps the conversation behind the view, the way every route does", () => {
    useAppStore.getState().openProjectView("p2");
    expect(useAppStore.getState().activeConversationId).toBe("p1a");
    useAppStore.getState().setView("chat");
    expect(useAppStore.getState().view).toBe("chat");
    expect(useAppStore.getState().activeConversationId).toBe("p1a");
  });
});

describe("project instructions reach the prompt (PRJ-7)", () => {
  it("injects the project's instructions for a session in it", () => {
    useAppStore.setState((s) => ({
      projects: [folderless("p1", "Quotes are in EUR."), ...s.projects.filter((p) => p.id !== "p1")],
    }));
    const out = composeSystemPrompt("BASE", {
      conv: undefined,
      sessionState: undefined,
      toolsEnabled: false,
      ...projectPromptFor("p1a"),
    });
    expect(out).toContain("## Project: p1");
    expect(out).toContain("Quotes are in EUR.");
  });

  it("says nothing for a project with no instructions, so an old folder project pays no tokens", () => {
    const out = composeSystemPrompt("BASE", {
      conv: undefined,
      sessionState: undefined,
      toolsEnabled: false,
      ...projectPromptFor("p1a"),
    });
    expect(out).toBe("BASE");
  });

  it("says nothing for a loose chat", () => {
    useAppStore.setState((s) => ({
      projects: [folderless("p1", "Quotes are in EUR."), ...s.projects.filter((p) => p.id !== "p1")],
    }));
    const out = composeSystemPrompt("BASE", {
      conv: undefined,
      sessionState: undefined,
      toolsEnabled: false,
      ...projectPromptFor("loose"),
    });
    expect(out).toBe("BASE");
  });
});

/** The store's own `projectPrompt` is internal; this mirrors what it hands
 * `composeSystemPrompt`, which is the contract under test. */
function projectPromptFor(convId: string) {
  const s = useAppStore.getState();
  const projectId = s.conversations.find((c) => c.id === convId)?.projectId;
  const project = projectId ? s.projects.find((p) => p.id === projectId) : undefined;
  if (!project?.instructions) return {};
  return { projectName: project.name, projectInstructions: project.instructions };
}

describe("moving sessions between projects (PRJ-9)", () => {
  it("takes a session out without deleting it", async () => {
    await useAppStore.getState().moveSessionToProject("p1a", null);
    const s = useAppStore.getState();
    const moved = s.conversations.find((c) => c.id === "p1a")!;
    expect(moved.projectId).toBeNull();
    expect(moved.folderPath ?? null).toBeNull();
    expect(s.conversations).toHaveLength(4);
  });

  it("carries the target project's folder onto a session it joins", async () => {
    await useAppStore.getState().moveSessionToProject("loose", "p2");
    const moved = useAppStore.getState().conversations.find((c) => c.id === "loose")!;
    expect(moved.projectId).toBe("p2");
    expect(moved.folderPath).toBe("C:\\work\\p2");
  });
});

describe("archiving (PRJ-3)", () => {
  it("hides the project without touching its sessions", async () => {
    await useAppStore.getState().archiveProject("p1");

    const s = useAppStore.getState();
    expect(s.projects.map((p) => p.id)).toEqual(["p2"]);
    // The conversations are still there. Nothing on disk is touched either —
    // there is no delete, because the word would be read as "delete my code".
    expect(s.conversations.filter((c) => c.projectId === "p1")).toHaveLength(2);
  });
});
