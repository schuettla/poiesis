/**
 * @vitest-environment jsdom
 *
 * `EXP-UI-1`: an experiment that makes quick decisions says it will be careful
 * until it has been measured, can measure now, and links to its decisions in
 * Activity.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CalibrateProgress, CalibrationReport, DecidePolicyRow } from "../lib/api";

const decidePolicy = vi.fn<() => Promise<DecidePolicyRow[]>>();
const decideCalibration = vi.fn<() => Promise<CalibrationReport[]>>();
const calibrateDecide = vi.fn<(on: (p: CalibrateProgress) => void) => Promise<CalibrationReport[]>>();
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  experimentsSeen: () => Promise.resolve(),
  decidePolicy: () => decidePolicy(),
  decideCalibration: () => decideCalibration(),
  calibrateDecide: (on: (p: CalibrateProgress) => void) => calibrateDecide(on),
}));

import Experiments from "./Experiments";
import { useAppStore } from "../lib/store";
import { experiment } from "../lib/experiments.fixture";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const setView = vi.fn();

const row = (purpose: string, experimentId: string | null): DecidePolicyRow => ({
  purpose,
  label: purpose,
  background: false,
  skipped_if_not_ready: true,
  deadline_s: 1,
  mode: "assisted",
  impact: "visible",
  escalate: "none",
  act_at_calibrated: 0.85,
  act_at_uncalibrated: 0.95,
  below: "Keeps the tools on the menu.",
  experiment: experimentId,
  backend_override: null,
});
const report = (purpose: string, calibrated: boolean): CalibrationReport => ({
  purpose,
  label: purpose,
  fit: null,
  calibrated,
  ece_before: null,
  new_labels: 0,
});
const fastSlow = (over = {}) =>
  experiment({
    id: "fast-slow",
    title: "Fast & slow thinking",
    stage: "early",
    on: true,
    requires: [{ kind: "decision-model", label: "A decision model", met: true, fix_view: "models" }],
    ...over,
  });
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
const flush = () => act(async () => {});

async function mount(list = [fastSlow()]) {
  act(() => useAppStore.setState({ experiments: list }));
  await act(async () => root.render(<Experiments />));
  await flush();
}

beforeEach(() => {
  decidePolicy.mockReset().mockResolvedValue([row("tool-select", "fast-slow"), row("goal-met", null)]);
  decideCalibration.mockReset().mockResolvedValue([report("tool-select", false), report("goal-met", false)]);
  calibrateDecide.mockReset().mockResolvedValue([report("tool-select", true)]);
  setView.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ experiments: null, setView, activityFilter: null });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the first-on note", () => {
  it("says it will be careful while its own questions are unmeasured, and ignores other purposes", async () => {
    await mount();
    expect(container.querySelector(".exp-note")?.textContent).toContain("I'll be careful until I've measured");
    expect(button("Calibrate now")).toBeDefined();
  });

  it("is not there once its questions are measured, or while the experiment is off", async () => {
    decideCalibration.mockResolvedValue([report("tool-select", true), report("goal-met", false)]);
    await mount();
    expect(container.querySelector(".exp-note")).toBeNull();

    decideCalibration.mockResolvedValue([report("tool-select", false)]);
    await mount([fastSlow({ on: false })]);
    expect(container.querySelector(".exp-note")).toBeNull();
  });

  it("measures now, shows the progress, and goes away when it is done", async () => {
    let progress: (p: CalibrateProgress) => void = () => {};
    let finish: () => void = () => {};
    calibrateDecide.mockImplementation((on) => {
      progress = on;
      return new Promise((resolve) => {
        finish = () => resolve([report("tool-select", true)]);
      });
    });
    await mount();
    await act(async () => button("Calibrate now")!.click());
    expect(button("Starting…")?.disabled).toBe(true);
    await act(async () => progress({ purpose: "tool-select", done: 12, total: 40 }));
    expect(button("Measuring 12 of 40…")).toBeDefined();
    decideCalibration.mockResolvedValue([report("tool-select", true)]);
    await act(async () => finish());
    await flush();
    expect(container.querySelector(".exp-note")).toBeNull();
  });

  it("says why when measuring fails, and Later puts it away", async () => {
    calibrateDecide.mockRejectedValue("The decision model isn't ready.");
    await mount();
    await act(async () => button("Calibrate now")!.click());
    expect(container.querySelector(".exp-error")?.textContent).toBe("The decision model isn't ready.");
    await act(async () => button("Later")!.click());
    expect(container.querySelector(".exp-note")).toBeNull();
  });
});

describe("the link to its decisions", () => {
  it("opens Activity narrowed to the experiment", async () => {
    await mount();
    await act(async () => button("Show them in Activity")!.click());
    expect(useAppStore.getState().activityFilter).toBe("experiment:fast-slow");
    expect(setView).toHaveBeenCalledWith("activity");
  });

  it("is only on a card that decides", async () => {
    await mount([experiment({ id: "workspace", on: true })]);
    expect(button("Show them in Activity")).toBeUndefined();
    expect(container.querySelector(".exp-note")).toBeNull();
    expect(decidePolicy).not.toHaveBeenCalled();
  });
});
