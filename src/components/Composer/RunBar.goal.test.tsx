/**
 * @vitest-environment jsdom
 *
 * `GOL-UI-1`: the goal is a segment of the run bar and a chip beside the footer,
 * says where I am in it while it runs and how it ended after, and can be stopped
 * from either.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RunBar from "./RunBar";
import ModeChips from "./ModeChips";
import { useAppStore } from "../../lib/store";
import { newGoal, type Goal } from "../../lib/goal";
import type { RunSummary } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CONV = "c1";
const stopGoal = vi.fn();

let container: HTMLDivElement;
let root: Root;

const summary = (over: Partial<RunSummary> = {}): RunSummary => ({
  runId: "r1",
  startedAt: 1000,
  live: true,
  files: [],
  costUsd: null,
  localRun: false,
  ...over,
});

const goal = (over: Partial<Goal> = {}): Goal => ({
  ...newGoal("make the tests pass", "npm test exits 0"),
  ...over,
});

function put(over: Partial<ReturnType<typeof useAppStore.getState>>) {
  act(() => useAppStore.setState(over));
}

beforeEach(() => {
  stopGoal.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: CONV,
    conversations: [{ id: CONV, title: "t", updatedAt: 0, messages: [] }],
    runSummaries: { [CONV]: summary() },
    subRuns: {},
    changeSets: {},
    pendingQuestion: null,
    busy: false,
    goals: {},
    stopGoal,
    toolsEnabled: true,
    workspaceMode: false,
  });
  act(() =>
    root.render(
      <>
        <RunBar />
        <ModeChips />
      </>
    )
  );
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const line = () => container.querySelector(".run-bar-line")?.textContent ?? "";
const chips = () => Array.from(container.querySelectorAll(".mode-chip-label")).map((c) => c.textContent);

describe("while it works", () => {
  it("is the first thing on the bar, with its round", () => {
    put({
      goals: { [CONV]: goal({ round: 2 }) },
      runSummaries: { [CONV]: summary({ files: ["a.ts"] }) },
    });
    expect(line()).toMatch(/^◎ make the tests pass · round 2 of 5/);
    expect(line()).toContain("I changed 1 file");
  });

  it("says it is checking between rounds", () => {
    put({ goals: { [CONV]: goal({ checking: true }) } });
    expect(line()).toContain("checking");
  });

  it("offers the last check's next step as its title", () => {
    put({
      goals: { [CONV]: goal({ lastCheck: { met: false, evidence: "", next: "2 tests still fail" } }) },
    });
    const link = container.querySelector<HTMLButtonElement>(".run-bar-link");
    expect(link?.title).toBe("2 tests still fail");
  });

  it("Stop goal stops it", () => {
    put({ goals: { [CONV]: goal() } });
    const stop = Array.from(container.querySelectorAll<HTMLButtonElement>(".run-bar-act")).find(
      (b) => b.textContent === "Stop goal"
    );
    act(() => stop!.click());
    expect(stopGoal).toHaveBeenCalledWith(CONV);
  });

  it("is a chip too, which can be taken off", () => {
    put({ goals: { [CONV]: goal() } });
    expect(chips()).toEqual(["Goal: make the tests pass"]);
    act(() => container.querySelector<HTMLButtonElement>(".mode-chip-x")!.click());
    expect(stopGoal).toHaveBeenCalledWith(CONV);
  });

  it("a long goal is cut, not allowed to take over the line", () => {
    put({ goals: { [CONV]: goal({ text: "x".repeat(120) }) } });
    expect(line().length).toBeLessThan(110);
    expect(line()).toContain("…");
  });
});

describe("after it ends", () => {
  it("met: Done, with what shows it, until the next send", () => {
    put({
      runSummaries: { [CONV]: summary({ live: false, stopReason: "completed" }) },
      goals: {
        [CONV]: goal({ status: "met", lastCheck: { met: true, evidence: "npm test exited 0", next: "" } }),
      },
    });
    expect(line()).toBe("◆ Done: npm test exited 0");
    // Nothing to stop, and no chip.
    expect(container.textContent).not.toContain("Stop goal");
    expect(chips()).toEqual([]);
  });

  it("exhausted: says it did not get there, and what was left", () => {
    put({
      runSummaries: { [CONV]: summary({ live: false, stopReason: "completed" }) },
      goals: {
        [CONV]: goal({
          status: "exhausted",
          round: 5,
          lastCheck: { met: false, evidence: "", next: "2 tests still fail" },
        }),
      },
    });
    expect(line()).toContain("not reached in 5 rounds");
    expect(container.querySelector<HTMLButtonElement>(".run-bar-link")?.title).toBe("2 tests still fail");
  });

  it("stopped: says so, plainly", () => {
    put({
      runSummaries: { [CONV]: summary({ live: false, stopReason: "aborted" }) },
      goals: { [CONV]: goal({ status: "stopped" }) },
    });
    expect(line()).toContain("stopped");
  });

  it("never shows a step count or a budget, however long the goal runs", () => {
    put({ goals: { [CONV]: goal({ round: 3 }) } });
    expect(container.textContent ?? "").not.toMatch(/step|of \d+ steps/i);
  });

  it("another chat's goal is not on this chat's bar", () => {
    put({ goals: { other: goal() } });
    expect(container.querySelector(".run-bar")).toBeNull();
    expect(chips()).toEqual([]);
  });
});
