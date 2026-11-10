/**
 * @vitest-environment jsdom
 *
 * `WRK-12`, `WRK-13`: where the goal used to be, the run bar says how much of what
 * done means is met, and while another way of working waits for the end of the item
 * it says which.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RunBar from "./RunBar";
import { useAppStore } from "../../lib/store";
import type { RunSummary } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CONV = "c1";
let container: HTMLDivElement;
let root: Root;
const requestComposer = vi.fn();

const summary = (over: Partial<RunSummary> = {}): RunSummary => ({
  runId: "r1",
  startedAt: 1000,
  live: true,
  files: [],
  costUsd: null,
  localRun: false,
  ...over,
});

beforeEach(() => {
  requestComposer.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: CONV,
    conversations: [{ id: CONV, title: "t", updatedAt: 0, messages: [] }],
    subRuns: {},
    changeSets: {},
    pendingQuestion: null,
    busy: false,
    requestComposer,
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const text = () => [...container.querySelectorAll(".run-bar-link")].map((b) => b.textContent);
const criteria = (met: (boolean | null)[]) => met.map((m, n) => ({ text: `criterion ${n + 1}`, met: m }));

describe("criteria on the run bar (WRK-12)", () => {
  it("says how many are met, live and after the run", () => {
    const plan = { revisions: 0, items: [], criteria: criteria([true, false, null]) };
    act(() => useAppStore.setState({ runSummaries: { [CONV]: summary({ plan }) } }));
    act(() => root.render(<RunBar />));
    expect(text()).toEqual(["◎ 1 of 3 criteria met"]);

    act(() => useAppStore.setState({ runSummaries: { [CONV]: summary({ plan, live: false }) } }));
    expect(text()).toEqual(["◎ 1 of 3 criteria met"]);
  });

  it("is quiet while the criteria are still waiting for the person's yes", () => {
    const plan = { revisions: 0, awaiting_approval: true, items: [], criteria: criteria([null, null]) };
    act(() => useAppStore.setState({ runSummaries: { [CONV]: summary({ plan }) } }));
    act(() => root.render(<RunBar />));
    expect(text()).toEqual([]);
  });

  it("says what is missing on hover, when a criterion was not met", () => {
    const plan = {
      revisions: 0,
      items: [],
      criteria: [{ text: "Excel opens it", met: false, why: "it opens with one column" }],
    };
    act(() => useAppStore.setState({ runSummaries: { [CONV]: summary({ plan }) } }));
    act(() => root.render(<RunBar />));
    expect(container.querySelector(".run-bar-link")?.getAttribute("title")).toBe("it opens with one column");
  });
});

describe("a switch that waits for the end of the item (WRK-13)", () => {
  it("is said while the run is live, and opens the menu", () => {
    act(() =>
      useAppStore.setState({
        runSummaries: { [CONV]: summary({ switching: { to: "command-and-control", name: "Command & Control" } }) },
      })
    );
    act(() => root.render(<RunBar />));
    expect(text()).toEqual(["Command & Control after this item"]);
    act(() => container.querySelector<HTMLButtonElement>(".run-bar-link")!.click());
    expect(requestComposer).toHaveBeenCalledWith("/style ");
  });

  it("is gone when the run is over", () => {
    act(() =>
      useAppStore.setState({
        runSummaries: { [CONV]: summary({ live: false, switching: { to: "lean", name: "Lean" } }) },
      })
    );
    act(() => root.render(<RunBar />));
    expect(text()).toEqual([]);
  });
});
