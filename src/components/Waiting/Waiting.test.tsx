/**
 * @vitest-environment jsdom
 *
 * `TEAM-19`, `TEAM-20`: what runs nobody watched left for the person. The sidebar says it, the
 * dialog lists it by chat, and when the last entry of a chat is answered the chat goes on.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChangeProposal } from "../../lib/api";

const calls = vi.hoisted(() => ({
  resolved: [] as { id: string; accept: boolean; text?: string }[],
  left: [] as unknown[],
  seen: [] as string[],
}));

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  resolveWaiting: async (id: string, accept: boolean, text?: string) => {
    calls.resolved.push({ id, accept, text });
    return accept ? `Yes to ${id}.` : `No to ${id}.`;
  },
  // What the backend would still hold: the entries not yet answered.
  listWaiting: async () => (calls.left as ChangeProposal[]).filter((p) => !calls.resolved.some((r) => r.id === p.id)),
  markSeen: async (id: string) => {
    calls.seen.push(id);
  },
  unreadSessions: async () => [],
}));

import Rail from "../Rail/Rail";
import { useAppStore } from "../../lib/store";
import type { Conversation } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const conv = (id: string, title: string): Conversation => ({
  id,
  title,
  updatedAt: Date.now() - 60_000,
  messages: [],
  projectId: null,
});

const entry = (id: string, session: string, over: Partial<ChangeProposal> = {}): ChangeProposal => ({
  id,
  target: "waiting-file",
  slug: null,
  proposed_text: "{}",
  rationale: "Delete old.txt — permanently delete",
  description: null,
  status: "pending",
  created_at: 1,
  conversation_id: session,
  session_id: session,
  author: "api",
  ...over,
});

let container: HTMLDivElement;
let root: Root;
const openSession = vi.fn(async () => {});
const sendMessage = vi.fn(async () => {});

function setup(waiting: ChangeProposal[], over: Record<string, unknown> = {}) {
  calls.resolved = [];
  calls.left = waiting;
  calls.seen = [];
  openSession.mockClear();
  sendMessage.mockClear();
  useAppStore.setState({
    personas: [],
    teams: [],
    conversations: [conv("c1", "Morning run"), conv("c2", "Other chat")],
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
    waiting,
    waitingNotes: {},
    waitingOpen: false,
    unreadIds: [],
    busy: false,
    openSession,
    sendMessage,
    ...over,
  } as never);
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

const render = () => act(() => root.render(<Rail />));
const click = (el: Element | null | undefined) => act(async () => (el as HTMLElement).click());
const button = (text: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === text)!;
const type = (el: HTMLTextAreaElement, value: string) =>
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
    set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });

describe("the sidebar", () => {
  it("has no row when nothing waits", () => {
    setup([]);
    render();
    expect(container.querySelector(".rail-waiting-row")).toBeNull();
  });

  it("says how many wait, and glows the chat they wait in", () => {
    setup([entry("e1", "c1"), entry("e2", "c1")]);
    render();
    expect(container.querySelector(".rail-waiting-row")?.textContent).toContain("Waiting for you · 2");
    const rows = Array.from(container.querySelectorAll<HTMLElement>(".chat-row"));
    const mine = rows.find((r) => r.querySelector(".chat-title")?.textContent === "Morning run")!;
    const other = rows.find((r) => r.querySelector(".chat-title")?.textContent === "Other chat")!;
    expect(mine.classList.contains("waiting")).toBe(true);
    expect(mine.querySelector('[aria-label="Waiting for you"]')).not.toBeNull();
    expect(other.classList.contains("waiting")).toBe(false);
  });

  it("puts a dot on a chat a run changed while nobody watched, and none on the others", () => {
    setup([], { unreadIds: ["c2"] });
    render();
    const rows = Array.from(container.querySelectorAll<HTMLElement>(".chat-row"));
    expect(rows.find((r) => r.querySelector(".chat-title")?.textContent === "Other chat")!.querySelector(".chat-unread")).not.toBeNull();
    expect(rows.find((r) => r.querySelector(".chat-title")?.textContent === "Morning run")!.querySelector(".chat-unread")).toBeNull();
  });
});

describe("the list", () => {
  it("is grouped by chat and keeps one size", async () => {
    setup([entry("e1", "c1"), entry("e2", "c2", { target: "waiting-ask", rationale: "api asks: Which table?" })]);
    render();
    await click(container.querySelector(".rail-waiting-row"));
    const dialog = document.querySelector(".waiting-dialog")!;
    expect(Array.from(dialog.querySelectorAll(".waiting-title")).map((t) => t.textContent)).toEqual(["Morning run", "Other chat"]);
    expect(dialog.querySelectorAll(".waiting-entry")).toHaveLength(2);
    expect(dialog.textContent).toContain("api would like to: Delete old.txt");
    // The error line is always there, so an error never changes the height.
    expect(dialog.querySelectorAll(".waiting-error")).toHaveLength(2);
  });

  it("makes a change only on Do it, and a question needs words first", async () => {
    setup([entry("e1", "c1"), entry("e2", "c2", { target: "waiting-ask", rationale: "api asks: Which table?" })]);
    render();
    await click(container.querySelector(".rail-waiting-row"));
    const send = button("Send answer");
    expect(send.disabled, "no answer, nothing to send").toBe(true);
    type(document.querySelector<HTMLTextAreaElement>(".waiting-answer")!, "orders");
    expect(button("Send answer").disabled).toBe(false);
    await click(button("Send answer"));
    expect(calls.resolved).toEqual([{ id: "e2", accept: true, text: "orders" }]);
    await click(button("Do it"));
    expect(calls.resolved[1]).toEqual({ id: "e1", accept: true, text: undefined });
  });
});

describe("going on", () => {
  it("goes on in a chat only when its last entry is answered, with what was decided", async () => {
    setup([entry("e1", "c1"), entry("e2", "c1", { rationale: "Delete other.txt" })]);
    render();
    await click(container.querySelector(".rail-waiting-row"));
    await click(button("Do it"));
    expect(sendMessage, "one entry still waits").not.toHaveBeenCalled();
    await click(button("Not now"));
    expect(openSession).toHaveBeenCalledWith("c1");
    expect(sendMessage).toHaveBeenCalledTimes(1);
    const said = sendMessage.mock.calls[0] as unknown as string[];
    expect(said[0]).toContain("- Yes to e1.");
    expect(said[0]).toContain("- No to e2.");
    expect(said[0]).toContain("Go on from there.");
  });

  it("does not go on when the person said no to everything", async () => {
    setup([entry("e1", "c1")]);
    render();
    await click(container.querySelector(".rail-waiting-row"));
    await click(button("Not now"));
    expect(calls.resolved).toEqual([{ id: "e1", accept: false, text: undefined }]);
    expect(sendMessage).not.toHaveBeenCalled();
  });
});
