/**
 * `HRN-UI-2`: steps that ran together are drawn together.
 *
 * The failure this guards against is subtle and bad: three concurrent reads
 * rendered as three ordinary rows read as three sequential reads, so the one
 * visible sign that the loop got faster is the one thing the UI loses.
 */
import { describe, expect, it } from "vitest";
import { bands } from "./Timeline";
import type { AgentStep } from "../../lib/types";

function step(id: string, group?: string): AgentStep {
  return { id, verb: "read", target: id, status: "done", ...(group ? { parallelGroup: group } : {}) };
}

describe("banding a timeline", () => {
  it("folds a batch into one band and leaves lone steps alone", () => {
    const out = bands([
      step("a"),
      step("b", "b"),
      step("c", "b"),
      step("d", "b"),
      step("e"),
    ]);
    expect(out.map((b) => b.steps.length)).toEqual([1, 3, 1]);
    expect(out[1].group).toBe("b");
  });

  it("keeps two batches apart even when they touch", () => {
    const out = bands([step("a", "a"), step("b", "a"), step("c", "c"), step("d", "c")]);
    expect(out).toHaveLength(2);
    expect(out.map((b) => b.group)).toEqual(["a", "c"]);
  });

  it("preserves call order inside and between bands", () => {
    const ids = bands([step("a"), step("b", "b"), step("c", "b")])
      .flatMap((b) => b.steps)
      .map((s) => s.id);
    expect(ids).toEqual(["a", "b", "c"]);
  });

  it("leaves an ordinary timeline untouched", () => {
    const out = bands([step("a"), step("b")]);
    expect(out.every((b) => b.group === null && b.steps.length === 1)).toBe(true);
  });
});
