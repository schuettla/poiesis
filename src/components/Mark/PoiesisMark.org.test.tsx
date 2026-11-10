/**
 * @vitest-environment jsdom
 *
 * `WRK-8`: the header mark shows the organization. Every shape it draws is a real
 * event, and every one is also in its label.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import PoiesisMark from "./PoiesisMark";
import { useAppStore } from "../../lib/store";
import type { RunSummary, SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const run = (over: Partial<RunSummary> = {}): RunSummary => ({
  runId: "r1",
  startedAt: 0,
  live: true,
  files: [],
  costUsd: null,
  localRun: true,
  style: { preset: "lean", name: "Lean" },
  ...over,
});

function put(summary: RunSummary | undefined, subRuns: Record<string, Partial<SubRun>> = {}) {
  act(() =>
    useAppStore.setState({
      activeConversationId: "c1",
      runSummaries: summary ? { c1: summary } : {},
      subRuns: subRuns as Record<string, SubRun>,
      presence: "active",
    })
  );
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => root.render(<PoiesisMark />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const mark = () => container.querySelector("svg.poiesis-mark")!;

describe("the mark shows the organization", () => {
  it("is the plain mark when nothing is organised", () => {
    put(undefined);
    expect(mark().querySelector(".mark-flow, .mark-bud, .mark-second")).toBeNull();
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — working");
  });

  it("takes Lean's shape while a run works that way, and says so", () => {
    put(run());
    expect(mark().classList.contains("org-lean")).toBe(true);
    expect(mark().querySelector(".mark-flow")).not.toBeNull();
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — working · working the Lean way");
  });

  it("buds a small cell off for each helper that is out, and none once they are done", () => {
    const helper = (status: string): Partial<SubRun> => ({ status: status as SubRun["status"], parentConversationId: "c1" });
    put(run(), { a: helper("running"), b: helper("running"), c: helper("done") });
    expect(mark().querySelectorAll(".mark-bud")).toHaveLength(2);
    expect(mark().getAttribute("aria-label")).toContain("2 helpers out");
    put(run(), { a: helper("done") });
    expect(mark().querySelectorAll(".mark-bud")).toHaveLength(0);
  });

  it("does not count another chat's helpers", () => {
    put(run(), { a: { status: "running", parentConversationId: "other" } });
    expect(mark().querySelectorAll(".mark-bud")).toHaveLength(0);
  });

  it("forms a second ring while someone is looking", () => {
    put(run({ looking: { item: 0 } }));
    expect(mark().querySelector(".mark-second")).not.toBeNull();
    expect(mark().getAttribute("aria-label")).toContain("one thing is being checked");
  });

  it("stands still while the line is stopped", () => {
    put(run({ stopped: { check: "cargo test" } }));
    expect(mark().classList.contains("org-stopped")).toBe(true);
    expect(mark().getAttribute("aria-label")).toContain("the line is stopped");
  });

  it("pulses once when an item ends, and fades and forms again on a fresh start", () => {
    put(run({ pulsedAt: Date.now() }));
    expect(mark().classList.contains("org-pulse")).toBe(true);
    put(run({ freshAt: Date.now() }));
    expect(mark().classList.contains("org-fresh")).toBe(true);
  });

  it("takes one slow breath when the task is done, and draws nothing of the run after", () => {
    put(run({ live: false, stopReason: "completed", endedAt: Date.now() }));
    expect(mark().classList.contains("org-done")).toBe(true);
    expect(mark().querySelector(".mark-flow, .mark-bud, .mark-second")).toBeNull();
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — working");
  });

  it("is Ad hoc's plain cell: nothing of an organization, whatever the run", () => {
    put(run({ style: { preset: "ad-hoc", name: "Ad hoc" } }));
    expect(mark().querySelector(".mark-flow")).toBeNull();
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — working");
  });
});

describe("each way of working has its own shape on the mark (WRK-12, WRK-14)", () => {
  const shapes: [string, string, string][] = [
    ["command-and-control", "Command & Control", ".mark-hub"],
    ["mission-command", "Mission Command", ".mark-aim"],
    ["swarm", "Swarm", ".mark-swarm"],
  ];

  it.each(shapes)("%s draws its shape while it works, and no other", (slug, name, shape) => {
    put(run({ style: { preset: slug, name } }));
    expect(mark().querySelector(shape)).not.toBeNull();
    for (const other of shapes.map((x) => x[2]).filter((x) => x !== shape)) {
      expect(mark().querySelector(other)).toBeNull();
    }
    expect(mark().querySelector(".mark-flow")).toBeNull();
    expect(mark().getAttribute("aria-label")).toContain(`working the ${name} way`);
  });

  it("draws none of them once the run is over", () => {
    put(run({ live: false, style: { preset: "swarm", name: "Swarm" } }));
    expect(mark().querySelector(".mark-hub, .mark-aim, .mark-swarm")).toBeNull();
  });
});

describe("what I changed on my own grows the mark (WRK-21)", () => {
  const layers = (n: number) => act(() => useAppStore.setState({ orgGrowth: { moments: [], layers: n } }));

  it("has no fine layer until I have changed how I work on my own", () => {
    put(undefined);
    layers(0);
    expect(mark().querySelectorAll(".mark-layer").length).toBe(0);
    expect(mark().getAttribute("aria-label")).not.toContain("how I work");
  });

  it("grows one for each change, up to three, and says so in its label", () => {
    put(undefined);
    layers(2);
    expect(mark().querySelectorAll(".mark-layer").length).toBe(2);
    expect(mark().getAttribute("aria-label")).toContain("I've changed how I work on my own");
    layers(9);
    expect(mark().querySelectorAll(".mark-layer").length).toBe(3);
  });

  it("is not drawn while a run is working: the layer forms at rest", () => {
    layers(2);
    put(run());
    expect(mark().querySelectorAll(".mark-layer").length).toBe(0);
  });
});
