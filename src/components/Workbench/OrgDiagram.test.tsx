/**
 * @vitest-environment jsdom
 *
 * `WRK-15`: the Coordination view. A shape for each style, drawn only from the
 * run's own events, with every state also in the sentence under it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AgentsPanel from "./AgentsPanel";
import { useAppStore } from "../../lib/store";
import type { PlanView } from "../../lib/api";
import type { RunSummary, SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CONV = "c1";
let container: HTMLDivElement;
let root: Root;
const openItem = vi.fn();

const preset = (slug: string, name: string, glyph = slug) => ({
  slug,
  name,
  line: "x",
  glyph,
  built_in: true,
  locked: false,
  version: 1,
  rules: [],
});

const summary = (over: Partial<RunSummary> = {}): RunSummary => ({
  runId: "r1",
  startedAt: 1000,
  live: true,
  files: [],
  costUsd: null,
  localRun: false,
  ...over,
});

const helper = (id: string, over: Partial<SubRun> = {}): SubRun =>
  ({
    runId: id,
    conversationId: `child-${id}`,
    parentConversationId: CONV,
    agent: "general",
    task: `task of ${id}`,
    status: "running",
    steps: [],
    text: "",
    startedAt: 2000,
    ...over,
  }) as SubRun;

function put(s: Partial<ReturnType<typeof useAppStore.getState>>) {
  act(() => useAppStore.setState(s as never));
}

beforeEach(() => {
  openItem.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: CONV,
    presets: [
      preset("lean", "Lean"),
      preset("command-and-control", "Command & Control"),
      preset("mission-command", "Mission Command"),
      preset("swarm", "Swarm"),
    ],
    subRuns: {},
    runSummaries: {},
    pendingQuestion: null,
    openItem,
    loadSubRuns: () => Promise.resolve(),
  } as never);
  act(() => root.render(<AgentsPanel conversationId={CONV} />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const svg = () => container.querySelector("svg.org-svg")!;
const sentence = () => container.querySelector(".org-sentence")?.textContent;
const plan = (over: Partial<PlanView> = {}): PlanView => ({
  revisions: 0,
  items: [
    { text: "Find the data", status: "done", review: { state: "checked", reason: "it ran" } },
    { text: "Export", status: "doing" },
    { text: "Button", status: "todo" },
  ],
  ...over,
});

describe("the Coordination view", () => {
  it("rests between tasks: the lead on its own, in words", () => {
    expect(svg().getAttribute("class")).toContain("shape-ad-hoc");
    expect(container.querySelectorAll(".org-node.item")).toHaveLength(0);
    expect(container.querySelector(".org-node.you")).not.toBeNull();
    expect(container.querySelector(".org-head")?.textContent).toContain("Ad hoc");
    expect(sentence()).toBe("Resting");
  });

  it("is a belt for Lean: the items in a line, each joined to the next", () => {
    put({ runSummaries: { [CONV]: summary({ style: { preset: "lean", name: "Lean" }, plan: plan({ style: "lean" }) }) } });
    expect(svg().getAttribute("class")).toContain("shape-lean");
    expect(container.querySelectorAll(".org-node.item")).toHaveLength(3);
    // The lead to the first item, and one between each pair.
    expect(container.querySelectorAll(".org-edge:not(.back)")).toHaveLength(3);
    expect(sentence()).toBe("working the Lean way");
    expect(container.querySelector(".org-now")?.textContent).toBe("Now: Export");
  });

  it("has you at the top for Command & Control, every item under me, and a line back to you", () => {
    put({ runSummaries: { [CONV]: summary({ style: { preset: "command-and-control", name: "Command & Control" }, plan: plan() }) } });
    expect(container.querySelector(".org-node.you")).not.toBeNull();
    // You to me, and me to each of the three items.
    expect(container.querySelectorAll(".org-edge:not(.back)")).toHaveLength(4);
    expect(container.querySelectorAll(".org-edge.back")).toHaveLength(1);
  });

  it("draws faint places for the items before there is a plan", () => {
    put({ runSummaries: { [CONV]: summary({ style: { preset: "lean", name: "Lean" } }) } });
    expect(container.querySelectorAll(".org-node.item")).toHaveLength(0);
    expect(container.querySelectorAll(".org-ghost")).toHaveLength(3);
  });

  it("puts the gate before done for Mission Command, and says what the look said when pressed", () => {
    const goal = plan({
      items: [],
      criteria: [{ text: "npm test exits 0", met: true }, { text: "no new warnings", met: false, why: "two new" }],
    });
    put({ runSummaries: { [CONV]: summary({ style: { preset: "mission-command", name: "Mission Command" }, plan: goal }) } });
    expect(container.querySelector(".org-node.gate")).not.toBeNull();
    expect(container.querySelector(".org-node.result")).not.toBeNull();
    expect(container.querySelectorAll(".org-node.item")).toHaveLength(2);
    act(() => container.querySelector<SVGGElement>(".org-node.gate")!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    const note = container.querySelector(".org-gate-note")!.textContent!;
    expect(note).toContain("Met: npm test exits 0.");
    expect(note).toContain("Not met: no new warnings (two new).");
  });

  it("lays the items on a board for Swarm, helpers around it, and opens a helper's transcript", () => {
    put({
      runSummaries: { [CONV]: summary({ style: { preset: "swarm", name: "Swarm" }, plan: plan() }) },
      subRuns: { h1: helper("h1"), h2: helper("h2", { status: "done" }) },
    });
    expect(container.querySelector(".org-board")).not.toBeNull();
    const helpers = container.querySelectorAll(".org-node.helper");
    expect(helpers).toHaveLength(2);
    expect(container.querySelectorAll(".org-node.helper.working")).toHaveLength(1);
    expect(sentence()).toBe("working the Swarm way · a helper is out");
    act(() => helpers[0].dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(openItem).toHaveBeenCalledWith({ kind: "run", id: "h1" });
  });

  it("stands still where the line is stopped and glows where you are being waited for", () => {
    put({
      runSummaries: {
        [CONV]: summary({ style: { preset: "command-and-control", name: "Command & Control" }, plan: plan(), stopped: { check: "cargo test" } }),
      },
      pendingQuestion: { convId: CONV } as never,
    });
    expect(container.querySelectorAll(".org-node.halt")).toHaveLength(1);
    expect(container.querySelector(".org-node.you.waiting")).not.toBeNull();
    expect(sentence()).toContain("the line is stopped");
    expect(sentence()).toContain("waiting for you");
  });

  it("jumps to the plan card from an item, and keeps no helper from before this run", () => {
    const board = document.createElement("section");
    board.className = "plan-card";
    let scrolled = false;
    board.scrollIntoView = () => {
      scrolled = true;
    };
    document.body.appendChild(board);
    put({
      runSummaries: { [CONV]: summary({ style: { preset: "lean", name: "Lean" }, plan: plan() }) },
      subRuns: { old: helper("old", { startedAt: -5000 }) },
    });
    expect(container.querySelectorAll(".org-node.helper")).toHaveLength(0);
    act(() => container.querySelector(".org-node.item")!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(scrolled).toBe(true);
    board.remove();
  });

  it("says how it worked once the run is over, as a record", () => {
    put({ runSummaries: { [CONV]: summary({ live: false, style: { preset: "lean", name: "Lean" }, plan: plan() }) } });
    expect(sentence()).toBe("Worked the Lean way");
  });
});
