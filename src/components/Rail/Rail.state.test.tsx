/**
 * @vitest-environment jsdom
 *
 * `UI-1` and `UI-2`: the mark at the start of each chat row, and the second line of a chat that
 * works or waits.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Rail from "./Rail";
import { useAppStore } from "../../lib/store";
import { NO_RAIL_FILTERS } from "../../lib/railView";
import type { ChangeProposal } from "../../lib/api";
import type { Conversation, RunSummary } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const conv = (id: string, over: Partial<Conversation> = {}): Conversation => ({
  id,
  title: id,
  updatedAt: Date.now(),
  messages: [],
  projectId: null,
  ...over,
});
const live = { runId: "r", startedAt: 0, live: true, files: [], costUsd: null, localRun: false } as RunSummary;

let container: HTMLDivElement;
let root: Root;

function setup(over: Record<string, unknown> = {}) {
  useAppStore.setState({
    conversations: [conv("idle"), conv("busy"), conv("asks"), conv("broke"), conv("fresh")],
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
    liveRuns: {},
    pendingPermissions: [],
    permissionChats: {},
    permissionAgents: {},
    outcomes: {},
    railFilters: NO_RAIL_FILTERS,
    railGroup: "date",
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
const row = (title: string) =>
  Array.from(container.querySelectorAll<HTMLElement>(".chat-row")).find(
    (r) => r.querySelector(".chat-title")?.textContent === title
  )!;
const label = (title: string) => row(title).querySelector(".chat-state [role=img]")?.getAttribute("aria-label") ?? null;

describe("the mark at the start of a chat row (UI-1)", () => {
  it("shows one mark per state, and none for a chat nothing happens in", () => {
    setup({
      runSummaries: {
        busy: live,
        broke: { ...live, live: false, stopReason: "error" },
      },
      waiting: [{ id: "w", target: "waiting-ask", session_id: "asks", rationale: "api asks: Which table?" } as ChangeProposal],
      unreadIds: ["broke", "fresh"],
    });
    render();
    expect(label("idle")).toBeNull();
    expect(label("busy")).toBe("Working");
    expect(label("asks")).toBe("Waiting for you");
    expect(label("broke")).toBe("Stopped with an error");
    expect(label("fresh")).toBe("Finished while you were away");
  });

  it("indents no row for a mark: an idle chat's title is the first thing in its line", () => {
    setup({ runSummaries: { busy: live } });
    render();
    expect(row("idle").querySelector(".chat-line")!.firstElementChild!.className).toBe("chat-title");
    expect(row("busy").querySelector(".chat-line")!.firstElementChild!.className).toBe("chat-title");
  });

  it("no longer draws the text glyph in a row", () => {
    setup({
      waiting: [{ id: "w", target: "waiting-ask", session_id: "asks", rationale: "x" } as ChangeProposal],
      runSummaries: { busy: live },
    });
    render();
    expect(container.querySelector(".chat-list")!.textContent).not.toContain("◌");
    expect(container.querySelector(".chat-waiting, .chat-unread")).toBeNull();
  });

  it("changes when the state changes", () => {
    setup();
    render();
    expect(label("busy")).toBeNull();
    act(() => useAppStore.setState({ runSummaries: { busy: live } } as never));
    expect(label("busy")).toBe("Working");
  });
});

describe("the second line (UI-2)", () => {
  it("shows for a chat that works or waits, and not for the others", () => {
    setup({
      runSummaries: { busy: live },
      pendingQuestion: { runId: "r1", id: "q", convId: "asks", messageId: "m", question: "Which table?", options: [], multi: false },
      unreadIds: ["fresh"],
    });
    render();
    expect(row("busy").querySelector(".chat-now")).not.toBeNull();
    expect(row("asks").querySelector(".tick-text.in")?.textContent).toBe("Which table?");
    expect(row("idle").querySelector(".chat-now")).toBeNull();
    expect(row("fresh").querySelector(".chat-now")).toBeNull();
  });

  it("follows the running step", () => {
    const step = (verb: string, target: string) => ({ id: "s", verb, target, status: "running" });
    const withStep = (s: object) =>
      conv("busy", { messages: [{ id: "m", role: "assistant", text: "", streaming: true, steps: [s] } as never] });
    setup({ conversations: [withStep(step("searching web", "competitor pricing"))], runSummaries: { busy: live } });
    render();
    const line = () => row("busy").querySelector(".tick-text.in")?.textContent;
    expect(line()).toBe("searching web competitor pricing");
    act(() => useAppStore.setState({ conversations: [withStep(step("reading", "pricing.md"))] } as never));
    expect(line()).toBe("reading pricing.md");
    // The old words are still on their way out.
    expect(row("busy").querySelector(".tick-text.out")?.textContent).toBe("searching web competitor pricing");
  });

  it("cuts a long question at a word", () => {
    const long = "Which of the three customer tables should I change, the old one or the one the import made last week?";
    setup({ pendingQuestion: { runId: "r1", id: "q", convId: "asks", messageId: "m", question: long, options: [], multi: false } });
    render();
    const text = row("asks").querySelector(".tick-text.in")!.textContent!;
    expect(text.endsWith("…")).toBe(true);
    expect(text.length).toBeLessThanOrEqual(61);
    expect(long.startsWith(text.slice(0, -1))).toBe(true);
  });
});

const going = (convId: string, ago: number) => ({ runId: `r-${convId}`, convId, startedAt: Date.now() - ago, thinking: "" });
const answering = (id: string, verb: string, target: string) =>
  conv(id, {
    messages: [
      { id: "m", role: "assistant", text: "", streaming: true, steps: [{ id: "s", verb, target, status: "running" }] } as never,
    ],
  });

describe("two chats working at once (PAR-9)", () => {
  const both = (over: Record<string, unknown> = {}) =>
    setup({
      conversations: [answering("trip", "Reading", "itinerary.md"), answering("login", "Running", "npm test"), conv("idle")],
      runSummaries: { trip: live, login: live },
      liveRuns: { trip: going("trip", 72_000), login: going("login", 7_000) },
      ...over,
    });
  const line = (title: string) => row(title).querySelector(".tick-text.in")?.textContent;
  const time = (title: string) => row(title).querySelector(".chat-now-time")?.textContent;

  it("shows each row its own second line and time", () => {
    both();
    render();
    expect([line("trip"), time("trip")]).toEqual(["Reading itinerary.md", "1:12"]);
    expect([line("login"), time("login")]).toEqual(["Running npm test", "0:07"]);
    expect(row("idle").querySelector(".chat-now")).toBeNull();
  });

  it("marks only the chat that asks, which then has no clock", () => {
    both({ pendingPermissions: [{ id: "p", summary: "Delete build/?" }], permissionChats: { p: "login" } });
    render();
    expect(label("login")).toBe("Waiting for you");
    expect(line("login")).toBe("Delete build/?");
    expect(time("login")).toBeUndefined();
    expect(label("trip")).toBe("Working");
  });

  it("gives a chat that waits for a folder its own mark and the reason, with no clock", () => {
    both({ runSummaries: { trip: live, login: { ...live, waitingOn: "Waiting: trip is changing files." } } });
    render();
    expect(label("login")).toBe("Waiting on something else");
    expect(line("login")).toBe("Waiting: trip is changing files.");
    expect(time("login")).toBeUndefined();
  });
});

describe("the Status filter (PAR-11)", () => {
  const titles = () => Array.from(container.querySelectorAll(".chat-row .chat-title")).map((t) => t.textContent);
  const click = (el: Element) => act(() => void el.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  const openStatus = () => {
    click(container.querySelector(".rail-more")!);
    const parent = Array.from(document.querySelectorAll(".row-menu-parent")).find((b) => b.textContent!.startsWith("Status\u2026"))!;
    click(parent);
  };
  const option = (text: string) =>
    Array.from(document.querySelectorAll<HTMLButtonElement>(".rail-flyout .rail-opt")).find(
      (b) => b.querySelector(".row-menu-label")!.textContent === text
    )!;
  const chats = (over: Record<string, unknown> = {}) =>
    setup({
      conversations: [conv("runs"), conv("asks"), conv("stopped"), conv("rest")],
      runSummaries: { runs: live },
      waiting: [{ id: "w", target: "waiting-ask", session_id: "asks", rationale: "x" } as ChangeProposal],
      outcomes: { stopped: "aborted" },
      ...over,
    });

  it("keeps the flyout open while states are picked, and shows every picked kind", () => {
    chats();
    render();
    openStatus();
    click(option("Running"));
    expect(titles()).toEqual(["runs"]);
    expect(option("Waiting for you")).toBeDefined();
    click(option("Waiting for you"));
    expect(titles()).toEqual(["runs", "asks"]);
    expect(option("Running").getAttribute("aria-checked")).toBe("true");
    expect(container.querySelector(".rail-filter-chip")!.textContent).toContain("Running or Waiting for you");
  });

  it("picks the three live states with one click", () => {
    chats();
    render();
    openStatus();
    click(option("Working or waiting now"));
    expect(useAppStore.getState().railFilters.status).toEqual(["running", "needs-you", "held"]);
    expect(option("Working or waiting now").getAttribute("aria-checked")).toBe("true");
    click(option("Working or waiting now"));
    expect(useAppStore.getState().railFilters.status).toEqual([]);
  });

  it("keeps a read chat whose last answer was stopped under Stopped early", () => {
    chats({ railFilters: { ...NO_RAIL_FILTERS, status: ["stopped"] } });
    render();
    expect(titles()).toEqual(["stopped"]);
  });

  it("keeps the chat on screen after its run ends, until another is opened", () => {
    chats({ activeConversationId: "runs", railFilters: { ...NO_RAIL_FILTERS, status: ["running"] } });
    render();
    expect(titles()).toEqual(["runs"]);
    act(() => useAppStore.setState({ runSummaries: { runs: { ...live, live: false } } } as never));
    expect(titles()).toEqual(["runs"]);
    act(() => useAppStore.setState({ activeConversationId: "rest" } as never));
    expect(titles()).toEqual([]);
    expect(container.querySelector(".rail-filter-empty")!.textContent).toBe("No chats are running.");
    click(container.querySelector(".rail-filter-all")!);
    expect(titles()).toEqual(["runs", "asks", "stopped", "rest"]);
  });
});
