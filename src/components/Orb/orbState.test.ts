import { describe, expect, it } from "vitest";
import { orbForFloor, orbForPresence, orbForStep, orbForSubRun } from "./orbState";

/**
 * The orb is a second reading of a status the app already states in words, so
 * these pin the mapping from the verbs the backend actually writes.
 */
describe("orbForStep", () => {
  it.each([
    ["searched", "searching"],
    ["searching web", "searching"],
    ["looked up", "searching"],
    ["visited", "searching"],
    ["recalled", "searching"],
    ["worked out", "solving"],
    ["ran", "solving"],
    ["checked", "solving"],
    ["wrote", "composing"],
    ["edited", "composing"],
    ["generated", "composing"],
    ["remembered", "weaving"],
    ["planned", "shaping"],
    ["updated the plan", "shaping"],
    // `CPX-T1`: the agent acting on its own session, and asking you something.
    ["made room", "shaping"],
    ["asked you:", "listening"],
  ])("maps %s to %s", (verb, state) => {
    expect(orbForStep(verb)).toBe(state);
  });

  it("does not take a mode or schedule request for a question to you", () => {
    expect(orbForStep("asked to switch to")).toBe("working");
    expect(orbForStep("asked to schedule")).toBe("working");
    expect(orbForStep("suggested")).toBe("working");
  });

  it("tells the memory tool's recall from a search of past conversations", () => {
    expect(orbForStep("recalled memory")).toBe("weaving");
    expect(orbForStep("recalled")).toBe("searching");
  });

  it("falls back to plain work for a verb it does not know", () => {
    expect(orbForStep("read")).toBe("working");
    expect(orbForStep("clicked")).toBe("working");
    expect(orbForStep("")).toBe("working");
  });
});

describe("orbForPresence", () => {
  it("breathes at rest and shows the kind of work otherwise", () => {
    expect(orbForPresence("idle")).toBe("breathing");
    expect(orbForPresence("active")).toBe("working");
    expect(orbForPresence("reflecting")).toBe("weaving");
    expect(orbForPresence("healing")).toBe("shaping");
  });

  it("shapes while I tend to myself and listens while I wait for you (CPX-T1)", () => {
    expect(orbForPresence("tending")).toBe("shaping");
    expect(orbForPresence("listening")).toBe("listening");
  });
});

describe("orbForSubRun", () => {
  const step = (verb: string, status: "running" | "done") => ({ id: verb, verb, target: "", status });

  it("does not claim work for a child that is only waiting its turn", () => {
    expect(orbForSubRun({ status: "queued", steps: [] })).toBe("breathing");
  });

  it("shows a child that has taken no step as still joining", () => {
    expect(orbForSubRun({ status: "running", steps: [] })).toBe("connecting");
  });

  it("follows the step a child is on", () => {
    expect(orbForSubRun({ status: "running", steps: [step("read", "done"), step("searched", "running")] })).toBe(
      "searching"
    );
  });

  it("is plain work in the gap between steps", () => {
    expect(orbForSubRun({ status: "running", steps: [step("searched", "done")] })).toBe("working");
  });
});

describe("orbForFloor (VXP-1)", () => {
  it("says who has the floor", () => {
    expect(orbForFloor("user_speaking")).toBe("listening");
    expect(orbForFloor("thinking")).toBe("connecting");
    expect(orbForFloor("speaking")).toBe("composing");
    expect(orbForFloor("listening", 0)).toBe("listening");
  });

  it("breathes once nobody has spoken for two seconds", () => {
    expect(orbForFloor("listening", 1999)).toBe("listening");
    expect(orbForFloor("listening", 2000)).toBe("breathing");
  });

  it("shows the work while Poiesis is working, and the voice while it speaks", () => {
    expect(orbForFloor("thinking", 0, "searched the web")).toBe("searching");
    expect(orbForFloor("thinking", 0, "ran a script")).toBe("solving");
    expect(orbForFloor("speaking", 0, "searched the web")).toBe("composing");
  });
});
