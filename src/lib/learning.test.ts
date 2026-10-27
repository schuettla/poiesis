import { describe, expect, it } from "vitest";
import type { LearningNumbers, LearningReport } from "./api";
import { compactChars, learningLines } from "./learning";

const numbers = (over: Partial<LearningNumbers> = {}): LearningNumbers => ({
  repeat_mistakes: 2,
  told_twice: 1,
  always_on_chars: 1100,
  recall_precision: null,
  stale_per_100: null,
  ...over,
});

describe("learningLines", () => {
  it("says nothing until there are two weeks of history", () => {
    expect(learningLines(null)).toBeNull();
    expect(learningLines({ current: numbers(), previous: null, enough_history: false })).toBeNull();
  });

  it("shows this month against last month", () => {
    const report: LearningReport = {
      current: numbers(),
      previous: numbers({ repeat_mistakes: 5, told_twice: 3, always_on_chars: 2400 }),
      enough_history: true,
    };
    expect(learningLines(report)).toEqual([
      "Mistakes I repeated: 2 (last month 5)",
      "Things you had to tell me twice: 1 (last month 3)",
      "Memory I carry in every message: 1.1k characters (last month 2.4k)",
      "Fixes I made that held up: not yet",
    ]);
  });

  it("leaves out the comparison when there is no earlier month", () => {
    const lines = learningLines({ current: numbers(), previous: null, enough_history: true });
    expect(lines?.[0]).toBe("Mistakes I repeated: 2");
  });
});

describe("compactChars", () => {
  it("keeps small numbers and rounds thousands to one decimal", () => {
    expect(compactChars(640)).toBe("640");
    expect(compactChars(1100)).toBe("1.1k");
    expect(compactChars(2000)).toBe("2k");
  });
});
