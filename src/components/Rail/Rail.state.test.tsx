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
    activeRun: null,
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
