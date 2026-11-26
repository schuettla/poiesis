/**
 * @vitest-environment jsdom
 *
 * A rail over a real history (hundreds of chats, thousands of helper jobs) must stay quick, and
 * a store update that changes nothing in the list must not walk every job once per row.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Rail from "./Rail";
import { useAppStore } from "../../lib/store";
import type { Conversation, SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CHATS = 400;
const JOBS = 4000;
const now = Date.now();
const conversations: Conversation[] = [];
const subRuns: Record<string, SubRun> = {};
for (let i = 0; i < CHATS; i++) conversations.push({ id: `c${i}`, title: `chat ${i}`, updatedAt: now - i * 60_000, messages: [], projectId: null });
for (let j = 0; j < JOBS; j++) {
  const parent = `c${j % CHATS}`;
  conversations.push({ id: `k${j}`, title: `job ${j}`, updatedAt: now - j * 1000, messages: [], projectId: null, parentConversationId: parent });
  subRuns[`r${j}`] = { runId: `r${j}`, conversationId: `k${j}`, parentConversationId: parent, agent: "general", task: "t", status: "done", steps: [], text: "", startedAt: now } as SubRun;
}

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  window.localStorage.clear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations, subRuns, personas: [], teams: [], projects: [], expandedProjects: [], activeConversationId: null,
    view: "chat", railCollapsed: false, changeProposals: [], reflectingIds: [], digestedIds: [], runningJob: null,
    paletteOpen: false, leaderFilter: null, waiting: [], pendingQuestion: null, queuedQuestions: [], runSummaries: {},
    unreadIds: [], activeRun: null,
  } as never);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the rail over a long history", () => {
  it("renders quickly, and an unrelated store update is cheap", () => {
    const t0 = performance.now();
    act(() => root.render(<Rail />));
    const first = performance.now() - t0;
    expect(container.querySelectorAll(".chat-row")).toHaveLength(CHATS);
    const t1 = performance.now();
    for (let i = 0; i < 20; i++) act(() => useAppStore.setState({ busy: i % 2 === 0 } as never));
    const updates = performance.now() - t1;
    console.log(`first render ${first.toFixed(0)}ms, 20 updates ${updates.toFixed(0)}ms`);
    expect(first).toBeLessThan(3000);
    expect(updates).toBeLessThan(1500);
  });
});
