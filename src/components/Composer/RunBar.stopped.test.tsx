/**
 * @vitest-environment jsdom
 *
 * `WRK-1`: while a check fails the run bar says so, first, and the segment goes
 * with the stop: a run that is over has no stopped line.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import RunBar from "./RunBar";
import { useAppStore } from "../../lib/store";
import type { RunSummary } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CONV = "c1";
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

beforeEach(() => {
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
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const text = () => [...container.querySelectorAll(".run-bar-link")].map((b) => b.textContent);

describe("the run bar and the line", () => {
  it("says which check stopped it, first, while the run is live", () => {
    act(() =>
      useAppStore.setState({
        runSummaries: {
          [CONV]: summary({
            stopped: { check: "cargo test" },
            plan: { revisions: 0, items: [{ text: "a", status: "doing" }] },
          }),
        },
      })
    );
    act(() => root.render(<RunBar />));
    expect(text()).toEqual(["stopped: cargo test failing", "plan 0 of 1"]);
  });

  it("has nothing to say about it once the run is over", () => {
    act(() =>
      useAppStore.setState({
        runSummaries: {
          [CONV]: summary({
            live: false,
            stopped: { check: "cargo test" },
            plan: { revisions: 0, items: [{ text: "a", status: "done" }] },
          }),
        },
      })
    );
    act(() => root.render(<RunBar />));
    expect(text()).toEqual(["plan done"]);
  });
});
