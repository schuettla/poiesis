import { describe, expect, it } from "vitest";
import { experimentOn, hasNewExperiment, workspaceAvailable } from "./experiments";
import { experiment } from "./experiments.fixture";
import { ALL_VIEWS, HUB_SECTIONS, isHubView } from "./types";

describe("experimentOn (EXP-1)", () => {
  it("reads the backend's state, and treats an unknown id as off", () => {
    const s = { experiments: [experiment({ on: true })] };
    expect(experimentOn(s, "workspace")).toBe(true);
    expect(experimentOn(s, "nope")).toBe(false);
    expect(experimentOn({ experiments: null }, "workspace")).toBe(false);
  });
});

describe("workspaceAvailable (EXP-3)", () => {
  it("is yes until the list has been read, so nobody's layout flickers away", () => {
    expect(workspaceAvailable({ experiments: null })).toBe(true);
  });

  it("follows the experiment once it has been read", () => {
    expect(workspaceAvailable({ experiments: [experiment({ on: false })] })).toBe(false);
    expect(workspaceAvailable({ experiments: [experiment({ on: true })] })).toBe(true);
    expect(workspaceAvailable({ experiments: [] })).toBe(false);
  });
});

describe("the new-experiment dot (EXP-UI-1)", () => {
  it("is on while any experiment has not been shown", () => {
    expect(hasNewExperiment({ experiments: null })).toBe(false);
    expect(hasNewExperiment({ experiments: [experiment({ is_new: true })] })).toBe(true);
    expect(hasNewExperiment({ experiments: [experiment({ is_new: false })] })).toBe(false);
  });
});

describe("the hub section (EXP-UI-1)", () => {
  it("sits last in the Poiesis group, and is a hub view", () => {
    const poiesis = HUB_SECTIONS.filter((s) => s.group === "poiesis").map((s) => s.view);
    expect(poiesis[poiesis.length - 1]).toBe("experiments");
    expect(ALL_VIEWS).toContain("experiments");
    expect(isHubView("experiments")).toBe(true);
  });
});
