import { describe, expect, it } from "vitest";
import { bucketFor, groupByBucket, shortTime } from "./time";

// Local wall-clock times, so the calendar boundaries are the machine's own.
const at = (y: number, mo: number, d: number, h = 12, mi = 0) =>
  new Date(y, mo - 1, d, h, mi).getTime();

describe("shortTime", () => {
  const now = at(2026, 9, 18, 14, 30);

  it("counts minutes, then hours, within today", () => {
    expect(shortTime(now - 20_000, now)).toBe("now");
    expect(shortTime(at(2026, 9, 18, 14, 18), now)).toBe("12m");
    expect(shortTime(at(2026, 9, 18, 9, 0), now)).toBe("5h");
  });

  it("names the weekday inside the last week, the date beyond it, in the UI's language", () => {
    expect(shortTime(at(2026, 9, 15), now)).toBe("Tue");
    expect(shortTime(at(2026, 3, 3), now)).toBe("Mar 3");
    expect(shortTime(at(2024, 3, 3), now)).toBe("Mar 24");
  });

  it("stays in minutes across midnight rather than jumping to a weekday", () => {
    expect(shortTime(at(2026, 9, 17, 23, 50), at(2026, 9, 18, 0, 10))).toBe("20m");
  });
});

describe("bucketFor", () => {
  const now = at(2026, 9, 18, 1, 0);

  it("uses calendar days, so 11pm last night is Yesterday at 1am", () => {
    expect(bucketFor(at(2026, 9, 17, 23, 0), now)).toBe("Yesterday");
    expect(bucketFor(at(2026, 9, 18, 0, 30), now)).toBe("Today");
  });

  it("falls through the wider windows in order", () => {
    expect(bucketFor(at(2026, 9, 13), now)).toBe("Previous 7 days");
    expect(bucketFor(at(2026, 8, 25), now)).toBe("Previous 30 days");
    expect(bucketFor(at(2026, 1, 2), now)).toBe("Older");
  });
});

describe("groupByBucket", () => {
  it("keeps bucket order and drops empty buckets", () => {
    const now = at(2026, 9, 18, 15);
    const groups = groupByBucket(
      [
        { id: "old", updatedAt: at(2025, 1, 1) },
        { id: "today", updatedAt: at(2026, 9, 18, 9) },
        { id: "yday", updatedAt: at(2026, 9, 17) },
      ],
      now
    );
    expect(groups.map((g) => g.label)).toEqual(["Today", "Yesterday", "Older"]);
    expect(groups[2].items.map((i) => i.id)).toEqual(["old"]);
  });
});
