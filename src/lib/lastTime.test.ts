import { describe, expect, it } from "vitest";
import { lastTimeLine, shortDate } from "./lastTime";

/** `EPI-UI-1`: the words of "Last time". */

const NOON_12_OCT = Date.UTC(2026, 9, 12, 12, 0, 0);

describe("Last time", () => {
  it("dates the line with day and month", () => {
    expect(shortDate(NOON_12_OCT, "en-GB")).toBe("12 Oct");
  });

  it("says what I did, in one line, after the date", () => {
    const line = lastTimeLine({ at: NOON_12_OCT, line: "I made the expense summary per category." });
    expect(line.startsWith("Last time (")).toBe(true);
    expect(line.endsWith(") I made the expense summary per category.")).toBe(true);
  });
});
