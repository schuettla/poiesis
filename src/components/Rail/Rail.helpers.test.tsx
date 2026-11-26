/**
 * @vitest-environment jsdom
 *
 * `UI-3`: the helpers of a chat, listed under it. A helper, an agent app's job and a team
 * member's run are conversations of their own; the rail used to leave them out.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Rail from "./Rail";
import { useAppStore } from "../../lib/store";
import type { Conversation, PendingQuestion, SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const conv = (id: string, over: Partial<Conversation> = {}): Conversation => ({
  id,
  title: id,
  updatedAt: 1_000,
  messages: [],
  projectId: null,
  ...over,
});
const kid = (id: string, at: number, parent = "lead"): Conversation =>
  conv(id, { parentConversationId: parent, updatedAt: at });

const run = (id: string, over: Partial<SubRun> = {}): SubRun => ({
  runId: id,
  conversationId: id,
  parentConversationId: "lead",
  agent: "general",
  task: "do it",
  status: "done",
  steps: [],
  text: "",
  startedAt: Date.now(),
  ...over,
});

let container: HTMLDivElement;
let root: Root;
const openSession = vi.fn(async () => {});

function setup(conversations: Conversation[], over: Record<string, unknown> = {}) {
  openSession.mockClear();
  useAppStore.setState({
    conversations,
    personas: [],
    teams: [],
    projects: [],
    expandedProjects: [],
    activeConversationId: null,
    view: "chat",
    railCollapsed: false,
    changeProposals: [],
    reflectingIds: [],
    digestedIds: [],
    runningJob: null,
    paletteOpen: false,
    leaderFilter: null,
    waiting: [],
    pendingQuestion: null,
    queuedQuestions: [],
    runSummaries: {},
    subRuns: {},
    unreadIds: [],
    activeRun: null,
    openSession,
    ...over,
  } as never);
}

beforeEach(() => {
  window.localStorage.clear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () => act(() => root.render(<Rail />));
const helpers = () =>
  Array.from(container.querySelectorAll<HTMLElement>(".helper-row")).map((r) => r.querySelector(".chat-title")?.textContent);
const twisty = () => container.querySelector<HTMLButtonElement>(".chat-twisty");
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());

describe("helpers under their chat (UI-3)", () => {
  it("lists a chat's helpers only after the chevron, newest first, and not as chats", () => {
    setup([conv("lead"), kid("older", 10), kid("newer", 20)]);
    render();
    expect(twisty()).not.toBeNull();
    expect(twisty()!.getAttribute("aria-expanded")).toBe("false");
    expect(helpers()).toEqual([]);
    // They are not chats of their own in the list.
    expect(Array.from(container.querySelectorAll(".chat-row .chat-title")).map((t) => t.textContent)).toEqual(["lead"]);
    click(twisty());
    expect(helpers()).toEqual(["newer", "older"]);
    expect(twisty()!.getAttribute("aria-expanded")).toBe("true");
  });

  it("has no chevron for a chat without helpers", () => {
    setup([conv("lead")]);
    render();
    expect(twisty()).toBeNull();
  });

  it("opens by itself while a helper works, and keeps the person's own choice after a re-render", () => {
    setup([conv("lead"), kid("a", 10)], { subRuns: { a: run("a", { status: "running" }) } });
    render();
    expect(helpers()).toEqual(["a"]);
    // Closed by the person: it stays closed although a helper still works.
    click(twisty());
    expect(helpers()).toEqual([]);
    act(() => root.unmount());
    root = createRoot(container);
    render();
    expect(helpers()).toEqual([]);
    expect(JSON.parse(window.localStorage.getItem("poiesis.rail.helpers")!)).toEqual({ lead: false });
  });

  it("is open while the chat itself is the open one", () => {
    setup([conv("lead"), kid("a", 10)], { activeConversationId: "lead" });
    render();
    expect(helpers()).toEqual(["a"]);
  });

  it("forgets the choice of a chat that is gone", () => {
    window.localStorage.setItem("poiesis.rail.helpers", JSON.stringify({ lead: true, gone: true }));
    setup([conv("lead"), kid("a", 10)]);
    render();
    expect(JSON.parse(window.localStorage.getItem("poiesis.rail.helpers")!)).toEqual({ lead: true });
  });

  it("shows five, then one row with the rest, which shows all of them in place", () => {
    setup([conv("lead"), ...Array.from({ length: 7 }, (_, i) => kid(`k${i}`, i))]);
    render();
    click(twisty());
    expect(helpers()).toHaveLength(5);
    const more = container.querySelector<HTMLButtonElement>(".helper-more")!;
    expect(more.textContent).toBe("2 more");
    click(more);
    expect(helpers()).toHaveLength(7);
    expect(container.querySelector(".helper-more")).toBeNull();
  });

  it("reads a team member as its role and the title", () => {
    setup([conv("lead"), kid("a", 10)], { subRuns: { a: run("a", { member: "api" }) }, activeConversationId: "lead" });
    render();
    expect(helpers()).toEqual(["api: a"]);
  });

  it("shows the logo of an agent app that did the job", () => {
    setup([conv("lead"), kid("a", 10)], {
      subRuns: { a: run("a", { engine: { id: "codex", name: "Codex", logo: "codex.svg" } as never }) },
      activeConversationId: "lead",
    });
    render();
    expect(container.querySelector(".helper-row img")?.getAttribute("src")).toContain("agent-logos/codex.svg");
  });

  it("marks a helper that has a question for the person", () => {
    const asked: PendingQuestion = { runId: "a", id: "q", convId: "lead", messageId: "m", question: "Which one?", options: [], multi: false };
    setup([conv("lead"), kid("a", 10)], {
      subRuns: { a: run("a", { status: "running", agent: "Claude Code" }) },
      pendingQuestion: asked,
    });
    render();
    const mark = container.querySelector(".helper-row .chat-state [role=img]");
    expect(mark?.getAttribute("aria-label")).toBe("Waiting for you");
    expect(container.querySelector(".helper-row .tick-text.in")?.textContent).toBe("Which one?");
  });

  it("opens a helper's own stream as a tab of its chat, as the Coordination panel does, and leaves the chat open", () => {
    const openItem = vi.fn();
    setup([conv("lead"), kid("a", 10)], { activeConversationId: "lead", openItem, subRuns: { a: run("a") } });
    render();
    click(container.querySelector(".helper-row"));
    expect(openItem).toHaveBeenCalledWith({ kind: "run", id: "a", conversationId: "lead" });
    expect(openSession).not.toHaveBeenCalled();
    act(() => useAppStore.setState({ activeItemId: "run:a" } as never));
    expect(helpers()).toEqual(["a"]);
    expect(container.querySelector(".helper-row")!.getAttribute("aria-current")).toBe("page");
  });

  it("opens the chat itself for a helper with no job on record", () => {
    setup([conv("lead"), kid("a", 10)], { activeConversationId: "lead", expandedProjects: [] });
    window.localStorage.setItem("poiesis.rail.helpers", JSON.stringify({ lead: true }));
    render();
    click(container.querySelector(".helper-row"));
    expect(openSession).toHaveBeenCalledWith("a");
  });

  it("has the menu: Stop while it works, and the chat it came from", () => {
    const stop = vi.fn(async () => {});
    const openTurnOf = vi.fn(async () => {});
    setup([conv("lead"), kid("a", 10)], {
      subRuns: { a: run("a", { status: "running" }) },
      stopSubRun: stop,
      openTurnOf,
    });
    render();
    const item = (text: string) =>
      Array.from(container.querySelectorAll<HTMLButtonElement>(".helper-row .row-menu-item")).find((b) => b.textContent === text);
    click(container.querySelector(".helper-row .chat-more"));
    expect(item("Open the chat it came from")).toBeDefined();
    click(item("Stop"));
    expect(stop).toHaveBeenCalledWith("a");
    click(container.querySelector(".helper-row .chat-more"));
    click(item("Open the chat it came from"));
    expect(openTurnOf).toHaveBeenCalledWith("a");
  });

  it("offers no Stop for a helper that has ended", () => {
    setup([conv("lead"), kid("a", 10)], { subRuns: { a: run("a", { status: "done" }) }, activeConversationId: "lead" });
    render();
    click(container.querySelector(".helper-row .chat-more"));
    const items = Array.from(container.querySelectorAll(".helper-row .row-menu-item")).map((b) => b.textContent);
    expect(items).toEqual(["Open the chat it came from"]);
  });

  it("keeps a chat under a leader filter when one of its helpers matches", () => {
    const personas = [{ id: "p1", name: "Ada", look: {} }] as never;
    setup([conv("lead", { leader: "poiesis" }), kid("a", 10), conv("other", { leader: "poiesis" })], {
      personas,
      leaderFilter: "persona:p1",
    });
    act(() =>
      useAppStore.setState({
        conversations: [
          conv("lead", { leader: "poiesis" }),
          conv("a", { parentConversationId: "lead", updatedAt: 10, leader: "persona:p1" }),
          conv("other", { leader: "poiesis" }),
        ],
      } as never)
    );
    render();
    expect(Array.from(container.querySelectorAll(".chat-row .chat-title")).map((t) => t.textContent)).toEqual(["lead"]);
  });
});
