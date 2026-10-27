import { describe, expect, it } from "vitest";
import type { CalibrationReport, DecideStatus } from "../../lib/api";
import { barText, calibrationSummary, calibrationText, fitText, panelState, statusLine } from "./DecisionModel";

const status = (over: Partial<DecideStatus> = {}): DecideStatus => ({
  state: "idle",
  model_installed: true,
  model_name: "Clef-flash",
  gpu: null,
  vision: null,
  median_ms: null,
  error: null,
  ...over,
});

describe("fitText (DEC-UI-1)", () => {
  it("names the chat model it fits next to", () => {
    expect(fitText({ fit: "gpu" }, "Qwen3.5-4B")).toBe("Fits on your graphics card next to Qwen3.5-4B");
    expect(fitText({ fit: "gpu" }, null)).toBe("Fits on your graphics card");
  });

  it("says the processor means background only, and when it won't fit at all", () => {
    expect(fitText({ fit: "cpu" }, "Qwen3.5-4B")).toContain("background only");
    expect(fitText({ fit: "wont-fit" }, null)).toContain("Won't fit");
  });
});

describe("panelState (the badge on the Decision model panel in Models)", () => {
  it("says where quick decisions run, and that nothing is set up when there is no model", () => {
    expect(panelState({ backend: "cloud" }, undefined)).toEqual({ tone: "running", text: "Running in the cloud" });
    expect(panelState({ backend: "local" }, { name: "Clef-flash" })).toEqual({
      tone: "running",
      text: "Running on this computer",
    });
    expect(panelState({ backend: "local" }, undefined)).toEqual({ tone: "idle", text: "Not set up" });
    expect(panelState(null, undefined)).toEqual({ tone: "idle", text: "Not set up" });
  });
});

describe("statusLine (DEC-UI-2)", () => {
  it("is grey and says it starts when needed", () => {
    expect(statusLine(status())).toEqual({ tone: "idle", text: "Starts when needed" });
  });

  it("is amber while starting", () => {
    expect(statusLine(status({ state: "starting" }))).toEqual({ tone: "starting", text: "Starting…" });
  });

  it("is green with where it runs and the median time", () => {
    expect(statusLine(status({ state: "ready", gpu: true, median_ms: 38 }))).toEqual({
      tone: "running",
      text: "Ready · graphics card · 38 ms",
    });
    expect(statusLine(status({ state: "ready", gpu: false })).text).toBe("Ready · processor");
  });

  it("is red with the reason when it couldn't start", () => {
    const line = statusLine(status({ state: "failed", error: "it ran out of memory" }));
    expect(line.tone).toBe("failed");
    expect(line.text).toBe("Couldn't start: it ran out of memory");
  });

  it("says it is not set up when there is no model, whatever the engine says", () => {
    expect(statusLine(status({ model_installed: false, model_name: null })).text).toBe("Not set up");
    expect(statusLine(null).text).toBe("Not set up");
  });
});

const report = (over: Partial<CalibrationReport> = {}): CalibrationReport => ({
  purpose: "goal-met",
  label: "Is the goal reached?",
  fit: null,
  calibrated: false,
  ece_before: null,
  new_labels: 0,
  ...over,
});

describe("calibrationText (CTL-4)", () => {
  it("says a purpose has not been measured", () => {
    expect(calibrationText(report())).toBe("Not measured yet");
  });

  it("says calibrated, with the error before and after", () => {
    const r = report({ fit: { t: 1.8, ece: 0.04, n: 40, fitted_at: 0 }, calibrated: true, ece_before: 0.21 });
    expect(calibrationText(r)).toBe("Calibrated · error 0.04, was 0.21 · 40 examples");
  });

  it("says how many examples are missing, and when the fit is still off", () => {
    expect(calibrationText(report({ fit: { t: 1, ece: 0.01, n: 12, fitted_at: 0 } }))).toBe(
      "Not enough examples yet (12 of 40)"
    );
    expect(calibrationText(report({ fit: { t: 2, ece: 0.15, n: 60, fitted_at: 0 } }))).toBe(
      "Measured, still off · error 0.15 · 60 examples"
    );
  });
});

describe("calibrationSummary (CTL-4)", () => {
  const fitted = (calibrated: boolean) => report({ fit: { t: 1, ece: 0.05, n: 40, fitted_at: 0 }, calibrated });

  it("says nothing was measured, and what that means", () => {
    expect(calibrationSummary(null)).toContain("only make me more careful");
    expect(calibrationSummary([report(), report()])).toContain("Not measured yet");
  });

  it("counts the calibrated questions", () => {
    expect(calibrationSummary([fitted(true), fitted(false)])).toBe(
      "Measured for 1 of 2 questions. For the rest a quick decision can only make me more careful."
    );
    expect(calibrationSummary([fitted(true), fitted(true)])).toContain("every question");
  });
});

describe("barText (CTL-1)", () => {
  it("shows one number when the two bars are the same, both when they are not", () => {
    expect(barText({ act_at_calibrated: 0.5, act_at_uncalibrated: 0.5 })).toBe("0.50");
    expect(barText({ act_at_calibrated: 0.15, act_at_uncalibrated: 0.05 })).toBe("0.15 (0.05 until measured)");
  });
});
