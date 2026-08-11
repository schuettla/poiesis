/**
 * Plain-assert checks for the growth-ring grouping (PRES-4), run under
 * vitest (`npm test`).
 */

import { describe, it } from "vitest";
import { groupByWeek } from "./growth";

/** Minimal assert — matches vitest's contract (throw = fail) without pulling
 * in `expect`'s matcher surface for checks this simple. */
const assert = {
  ok(cond: unknown, msg?: string) {
    if (!cond) throw new Error(`assertion failed: ${msg ?? "expected truthy"}`);
  },
  equal(actual: unknown, expected: unknown, msg?: string) {
    if (actual !== expected) {
      throw new Error(
        `assertion failed: ${msg ?? ""} — got ${String(actual)}, want ${String(expected)}`
      );
    }
  },
};

const DAY = 24 * 60 * 60 * 1000;
const iso = (at: number) => new Date(at).toISOString().slice(0, 10);

describe("groupByWeek (PRES-4)", () => {
  it("buckets entries into one week per week lived, including the current one", () => {
    // Born three weeks and a day ago. Aligned to midnight, because entry
    // dates are day-granular (YYYY-MM-DD) while the birthday is a timestamp —
    // comparing the two unaligned would shift entries a bucket earlier.
    const born = Date.parse(iso(Date.now() - 22 * DAY));
    const entry = (daysAfterBirth: number) => ({ created: iso(born + daysAfterBirth * DAY) });

    const weeks = groupByWeek(
      [entry(0), entry(3), entry(8), entry(9), entry(10), entry(20)],
      born
    );
    assert.equal(weeks.length, 4, "one bucket per week lived, including the current one");
    assert.equal(weeks[0], 2, "week 0 holds the first two entries");
    assert.equal(weeks[1], 3, "week 1 holds the middle three");
    assert.equal(weeks[2], 1, "week 2 covers days 14-20");
    assert.equal(weeks[3], 0, "the current, still-empty week is a real week, not a gap");
  });

  it("gives a brand-new install exactly one week, however empty", () => {
    const fresh = groupByWeek([], Date.now());
    assert.equal(fresh.length, 1);
    assert.equal(fresh[0], 0);
  });

  it("never silently drops or crashes on pre-birthday or unparseable dates", () => {
    const born = Date.parse(iso(Date.now() - 22 * DAY));
    const odd = groupByWeek(
      [{ created: iso(born - 90 * DAY) }, { created: "" }, { created: "not a date" }],
      born
    );
    assert.equal(odd[0], 1, "an older-than-birth entry lands in week 0");
    assert.equal(
      odd.reduce((a, b) => a + b, 0),
      1,
      "undated entries are ignored, not counted into some week"
    );
  });
});
