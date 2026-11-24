import { describe, expect, it } from "vitest";
import { SpokenTurns } from "./turns";

describe("SpokenTurns (VTN-6)", () => {
  it("saves only the pieces that finished playing", () => {
    const t = new SpokenTurns();
    const g = t.begin();
    t.add(g, 0, "First sentence.", false);
    t.add(g, 1, "Second sentence.", false);
    t.add(g, 2, "Third sentence.", false);
    t.played(g, 0);
    t.played(g, 1);
    t.interrupt(g);
    expect(t.heard(g)).toBe("First sentence. Second sentence.");
    expect(t.wasInterrupted(g)).toBe(true);
  });

  it("does not count a spoken notice as part of the reply", () => {
    const t = new SpokenTurns();
    const g = t.begin();
    t.add(g, 0, "Let me look that up.", true);
    t.add(g, 1, "It is raining.", false);
    t.played(g, 0);
    t.played(g, 1);
    expect(t.heard(g)).toBe("It is raining.");
  });

  it("puts pieces in order even when they were played out of order", () => {
    const t = new SpokenTurns();
    const g = t.begin();
    t.add(g, 1, "two", false);
    t.add(g, 0, "one", false);
    t.played(g, 1);
    t.played(g, 0);
    expect(t.heard(g)).toBe("one two");
  });

  it("says playback is not over while a made piece has not played", () => {
    const t = new SpokenTurns();
    const g = t.begin();
    expect(t.pending(g)).toBe(0);
    t.add(g, 0, "a", false);
    t.add(g, 1, "b", false);
    t.played(g, 0);
    expect(t.pending(g)).toBe(1);
    t.played(g, 1);
    expect(t.pending(g)).toBe(0);
  });

  it("keeps replies apart and ignores pieces of replies it does not know", () => {
    const t = new SpokenTurns();
    const a = t.begin();
    const b = t.begin();
    expect(b).toBeGreaterThan(a);
    t.add(a, 0, "old", false);
    t.add(99, 0, "stray", false);
    t.played(99, 0);
    expect(t.heard(b)).toBe("");
    expect(t.heard(99)).toBe("");
    expect(t.heard(a)).toBe("");
    t.forget(a);
    expect(t.wasInterrupted(a)).toBe(false);
  });
});
