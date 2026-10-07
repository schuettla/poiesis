/**
 * @vitest-environment jsdom
 *
 * `RUN-T1c`, `RUN-T2`, `RUN-T3`: the run bar says what a run is doing and did,
 * says nothing when there is nothing to say, and never shows a limit as if it
 * were a status.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import RunBar, { costSegment } from "./RunBar";
import { useAppStore } from "../../lib/store";
import type { RunSummary, SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CONV = "c1";

let container: HTMLDivElement;
let root: Root;

function summary(over: Partial<RunSummary> = {}): RunSummary {
  return {
    runId: "r1",
    startedAt: 1000,
    live: true,
    files: [],
    costUsd: null,
    localRun: false,
    ...over,
  };
}

function agent(id: string, over: Partial<SubRun> = {}): SubRun {
  return {
    runId: id,
    conversationId: `child-${id}`,
    parentConversationId: CONV,
    agent: "researcher",
    task: "read",
    status: "running",
    steps: [],
    text: "",
    startedAt: 2000,
    ...over,
  };
}

function put(over: Partial<ReturnType<typeof useAppStore.getState>>) {
  act(() => useAppStore.setState(over));
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: CONV,
    conversations: [{ id: CONV, title: "t", updatedAt: 0, messages: [] }],
    runSummaries: {},
    subRuns: {},
    changeSets: {},
    pendingQuestion: null,
    busy: false,
  });
  act(() => root.render(<RunBar />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const line = () => container.querySelector(".run-bar-line")?.textContent ?? "";
const bar = () => container.querySelector(".run-bar");

describe("what shows (RUN-T2)", () => {
  it("is not there for a plain chat answer", () => {
    expect(bar()).toBeNull();
    put({ runSummaries: { [CONV]: summary({ live: false, stopReason: "completed" }) } });
    expect(bar()).toBeNull();
  });

  it("shows the plan as items done of items, not counting dropped ones", () => {
    put({
      runSummaries: {
        [CONV]: summary({
          plan: {
            revisions: 0,
            items: [
              { text: "a", status: "done" },
              { text: "b", status: "doing" },
              { text: "c", status: "todo" },
              { text: "d", status: "dropped" },
            ],
          },
        }),
      },
    });
    expect(line()).toContain("plan 1 of 3");
  });

  it("says a finished plan is done, and an interrupted one where it stopped", () => {
    const plan = (statuses: ("done" | "todo")[]) => ({
      revisions: 0,
      items: statuses.map((status, i) => ({ text: String(i), status })),
    });
    put({ runSummaries: { [CONV]: summary({ live: false, stopReason: "completed", plan: plan(["done", "done"]) }) } });
    expect(line()).toContain("plan done");
    put({ runSummaries: { [CONV]: summary({ live: false, stopReason: "max_steps", plan: plan(["done", "todo"]) }) } });
    expect(line()).toContain("stopped at 1 of 2");
  });

  it("counts changed files live by path, once each", () => {
    put({ runSummaries: { [CONV]: summary({ files: ["a.ts", "b.ts"] }) } });
    expect(line()).toContain("I changed 2 files");
    expect(container.textContent).not.toContain("Undo");
  });

  it("after the run, takes the file count from the Changes view, with Undo and Keep", () => {
    put({
      runSummaries: { [CONV]: summary({ live: false, stopReason: "completed", files: ["a.ts"] }) },
      changeSets: {
        [CONV]: {
          files: [{ path: "a.ts" }, { path: "b.ts" }, { path: "c.ts" }] as never,
          added: 0,
          removed: 0,
          since: 0,
          this_run: true,
        },
      },
    });
    expect(line()).toContain("I changed 3 files");
    const labels = Array.from(container.querySelectorAll(".run-bar-act")).map((b) => b.textContent);
    expect(labels).toEqual(["Undo", "Keep"]);
  });

  it("counts agents working, then reported back and still working", () => {
    put({
      runSummaries: { [CONV]: summary() },
      subRuns: { a: agent("a"), b: agent("b") },
    });
    expect(line()).toContain("2 agents working");
    put({
      runSummaries: { [CONV]: summary({ live: false, stopReason: "completed" }) },
      subRuns: { a: agent("a", { status: "done" }), b: agent("b", { status: "running" }) },
    });
    expect(line()).toContain("1 agent reported back · 1 still working");
  });

  it("ignores agents from an earlier run and from another chat", () => {
    put({
      runSummaries: { [CONV]: summary({ startedAt: 5000 }) },
      subRuns: {
        old: agent("old", { startedAt: 100 }),
        else: agent("else", { parentConversationId: "other", startedAt: 6000 }),
      },
    });
    expect(bar()).toBeNull();
  });

  it("never shows a step count or a budget, however busy the run", () => {
    put({
      runSummaries: {
        [CONV]: summary({
          costUsd: 0.04,
          files: ["a", "b", "c"],
          plan: { revisions: 0, items: [{ text: "x", status: "doing" }, { text: "y", status: "todo" }] },
        }),
      },
      subRuns: { a: agent("a") },
    });
    expect(container.textContent ?? "").not.toMatch(/step|of \d+ steps|max/i);
  });

  it("says I asked you something, first, while a question is open (AGC-3)", () => {
    put({
      runSummaries: { [CONV]: summary({ files: ["a.ts"] }) },
      pendingQuestion: {
        runId: "r1",
        id: "q1",
        convId: CONV,
        messageId: "m1",
        question: "Which?",
        options: [{ label: "A" }, { label: "B" }],
        multi: false,
      },
    });
    expect(line()).toMatch(/^I asked you something ↑/);
    expect(line()).toContain("I changed 1 file");
    put({ pendingQuestion: null });
    expect(line()).not.toContain("I asked you something");
  });

  it("offers Continue where I stopped after a run that ran out", () => {
    put({
      runSummaries: { [CONV]: summary({ live: false, stopReason: "max_steps", files: ["a"] }) },
    });
    expect(container.textContent).toContain("Continue where I stopped");
    put({ runSummaries: { [CONV]: summary({ live: false, stopReason: "completed", files: ["a"] }) } });
    expect(container.textContent).not.toContain("Continue where I stopped");
  });
});

describe("what it costs (RUN-T1c)", () => {
  const done = (usd: number | null) => agent("x", { status: "done", costUsd: usd });

  it("is the lead plus this run's agents", () => {
    expect(costSegment(0.04, false, [done(0.01), done(0.02)], false)?.text).toBe("$0.07");
  });

  it("says so live", () => {
    expect(costSegment(0.04, false, [], true)?.text).toBe("$0.04 so far");
  });

  it("one agent I cannot price gives '+ agents', never a partial total", () => {
    const seg = costSegment(0.04, false, [done(0.01), done(null)], true)!;
    expect(seg.text).toBe("$0.04 so far + agents");
    expect(seg.title).toBe("I can't price what one of my agents used.");
  });

  it("an agent that has not reported yet is also unknown", () => {
    expect(costSegment(0.04, false, [agent("x")], false)?.text).toBe("$0.04 + agents");
  });

  it("is absent when the lead cannot be priced", () => {
    expect(costSegment(null, false, [done(0.01)], false)).toBeNull();
  });

  it("says a local run is on this machine, once, instead of $0.00", () => {
    expect(costSegment(null, true, [], false)?.text).toBe("on this machine");
  });

  it("a very small cost is under a cent, not zero", () => {
    expect(costSegment(0.001, false, [], false)?.text).toBe("under $0.01");
  });
});

describe("how long it stays (RUN-T3)", () => {
  const seeded = () =>
    put({ runSummaries: { [CONV]: summary({ live: false, stopReason: "completed", files: ["a"] }) } });

  it("survives the run ending", () => {
    seeded();
    expect(bar()).not.toBeNull();
    put({ busy: false, activeRun: null });
    expect(bar()).not.toBeNull();
  });

  it("goes with the x", () => {
    seeded();
    act(() => container.querySelector<HTMLButtonElement>(".run-bar-x")!.click());
    expect(bar()).toBeNull();
  });

  it("is replaced by the next run", () => {
    seeded();
    put({ runSummaries: { [CONV]: summary({ runId: "r2", files: [] }) } });
    expect(bar()).toBeNull();
  });

  it("does not follow you to another chat", () => {
    put({
      conversations: [
        { id: CONV, title: "t", updatedAt: 0, messages: [] },
        { id: "c2", title: "u", updatedAt: 0, messages: [] },
      ],
    });
    seeded();
    act(() => {
      void useAppStore.getState().setActiveConversation("c2");
    });
    expect(useAppStore.getState().runSummaries[CONV]).toBeUndefined();
  });
});
