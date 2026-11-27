import { describe, expect, it } from "vitest";
import { clockText, cutAtWord, liveActivity, stepsDuration, stepTime, stepWords, writingCount } from "./liveWords";

describe("stepTime (UI-4)", () => {
  it("writes under a second as 0.4s, under a minute as 38s, else as 2m 41s", () => {
    expect(stepTime(400)).toBe("0.4s");
    expect(stepTime(0)).toBe("0.0s");
    expect(stepTime(1000)).toBe("1s");
    expect(stepTime(38_000)).toBe("38s");
    expect(stepTime(59_400)).toBe("59s");
    expect(stepTime(161_000)).toBe("2m 41s");
    expect(stepTime(3_600_000)).toBe("60m 0s");
  });
});

describe("clockText", () => {
  it("is m:ss", () => {
    expect(clockText(0)).toBe("0:00");
    expect(clockText(42_900)).toBe("0:42");
    expect(clockText(125_000)).toBe("2:05");
    expect(clockText(-5)).toBe("0:00");
  });
});

describe("cutAtWord (UI-2)", () => {
  it("leaves a short text alone and flattens its white space", () => {
    expect(cutAtWord("  Which\ntable? ")).toBe("Which table?");
  });
  it("cuts a long one at a word with an ellipsis, and a word with no space at the limit", () => {
    expect(cutAtWord("one two three four five", 12)).toBe("one two…");
    expect(cutAtWord("abcdefghijklmnop", 5)).toBe("abcde…");
  });
});

describe("stepWords and liveActivity", () => {
  it("say verb and target, trimmed", () => {
    expect(stepWords({ verb: "reading", target: "a.ts" })).toBe("reading a.ts");
    expect(stepWords({ verb: "thinking", target: "" })).toBe("thinking");
  });

  it("name the running tool, then thinking, then what the loop is waiting on", () => {
    const run = { thinking: "", plan: undefined };
    expect(liveActivity([{ status: "running", verb: "read", target: "x" }], run)).toBe("read x");
    expect(liveActivity([{ status: "done", verb: "read", target: "x" }], { thinking: "hm" })).toBe("thinking");
    expect(liveActivity([{ status: "done", verb: "read", target: "x" }], run)).toBe("waiting for the model");
    expect(liveActivity(undefined, null)).toBe("waiting for the model");
  });

  it("say a long tool call being written after a running tool and before thinking, with its count", () => {
    const writing = { thinking: "hm", writing: { label: "AI in Marketing", words: 1240 } };
    expect(liveActivity([], writing)).toBe("writing “AI in Marketing”");
    expect(liveActivity([], { thinking: "", writing: { label: null } })).toBe("writing");
    expect(writingCount([], writing)).toBe(1240);
    const running = [{ status: "running" as const, verb: "created", target: "x" }];
    expect(liveActivity(running, writing)).toBe("created x");
    expect(writingCount(running, writing)).toBeNull();
    expect(writingCount([], { thinking: "hm" } as never)).toBeNull();
  });

  it("puts the plan item being worked on in front", () => {
    const plan = { items: [{ status: "done", text: "a" }, { status: "doing", text: "sketch the layout" }] };
    expect(liveActivity([{ status: "running", verb: "read", target: "x" }], { thinking: "", plan })).toBe(
      "sketch the layout · read x"
    );
  });
});

describe("stepsDuration (UI-4)", () => {
  it("is the first start to the last end", () => {
    expect(stepsDuration([{ startedAt: 1000, endedAt: 1400 }, { startedAt: 1400, endedAt: 72_000 }])).toBe(71_000);
  });
  it("is null when any step has no times, or there are none", () => {
    expect(stepsDuration([{ startedAt: 1, endedAt: 2 }, {}])).toBeNull();
    expect(stepsDuration([])).toBeNull();
  });
});
