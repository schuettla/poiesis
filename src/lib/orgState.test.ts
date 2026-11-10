import { describe, expect, it } from "vitest";
import { itemDot, orgWords } from "./orgState";
import type { PlanItem } from "./api";

const item = (over: Partial<PlanItem>): PlanItem => ({ text: "x", status: "todo", ...over });

describe("what the organization is doing, in words (WRK-8)", () => {
  const quiet = { preset: null, live: true, helpers: 0, looking: false, branching: false, stopped: false };

  it("says nothing when nothing is going on, or when the run is over", () => {
    expect(orgWords(quiet)).toEqual([]);
    expect(orgWords({ ...quiet, live: false, preset: "Lean", helpers: 2, stopped: true })).toEqual([]);
  });

  it("reads like one sentence: the way, the helpers, what is being checked", () => {
    expect(orgWords({ ...quiet, preset: "Lean", helpers: 2, looking: true }).join(" · ")).toBe(
      "working the Lean way · 2 helpers out · one thing is being checked"
    );
    expect(orgWords({ ...quiet, helpers: 1 })).toEqual(["a helper is out"]);
    expect(orgWords({ ...quiet, branching: true, stopped: true })).toEqual(["two ways are being tried", "the line is stopped"]);
  });
});

describe("an item as a dot (WRK-9)", () => {
  it("has a shape for each state, and a word for it", () => {
    expect(itemDot(item({}))).toEqual({ glyph: "○", word: "waiting" });
    expect(itemDot(item({ status: "doing" }))).toEqual({ glyph: "◐", word: "in hand" });
    expect(itemDot(item({ status: "done" }))).toEqual({ glyph: "●", word: "done" });
    expect(itemDot(item({ status: "dropped" }))).toEqual({ glyph: "×", word: "dropped" });
  });

  it("shows a look under way and a send-back, and a checked item as done", () => {
    expect(itemDot(item({ status: "doing", review: { state: "checking" } })).glyph).toBe("◔");
    expect(itemDot(item({ status: "doing", review: { state: "sent_back", reason: "no test" } })).glyph).toBe("↺");
    expect(itemDot(item({ status: "done", review: { state: "checked" } })).glyph).toBe("●");
  });
});
