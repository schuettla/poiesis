import { describe, expect, it } from "vitest";
import { row } from "./inbox.fixture";
import { countRows, filterRows, isUrgent, kindHint, replyHint, statusLine, urgencyHint, whenText } from "./inbox";

describe("the filters (USE-UI-4)", () => {
  const rows = [
    row({ uid: 1, urgency: 4, urgency_p: 0.9, reply: 0.8 }),
    row({ uid: 2, urgency: 3, urgency_p: 0.7 }),
    row({ uid: 3, urgency: 2, urgency_p: 0.7, reply: 0.7 }),
    row({ uid: 4 }),
  ];

  it("counts the messages that need a reply and the urgent ones, 3 and 4 being urgent", () => {
    expect(countRows(rows)).toEqual({ all: 4, reply: 2, urgent: 2 });
    expect(isUrgent(row({ urgency: 3 }))).toBe(true);
    expect(isUrgent(row({ urgency: 2 }))).toBe(false);
    expect(isUrgent(row({ urgency: null }))).toBe(false);
  });

  it("narrows the list, and All is the whole list", () => {
    expect(filterRows(rows, "all")).toHaveLength(4);
    expect(filterRows(rows, "reply").map((r) => r.uid)).toEqual([1, 3]);
    expect(filterRows(rows, "urgent").map((r) => r.uid)).toEqual([1, 2]);
  });
});

describe("the words on a row", () => {
  it("says what was judged and how sure, on the urgency bar", () => {
    expect(urgencyHint(row({ urgency: 4, urgency_p: 0.87 }))).toBe("Probably urgent");
    expect(urgencyHint(row({ urgency: 3, urgency_p: 0.74 }))).toBe("Probably needs attention soon");
    expect(urgencyHint(row({ urgency: 1, urgency_p: 0.6 }))).toBe("Probably not urgent");
    expect(urgencyHint(row({ urgency: null }))).toBeNull();
  });

  it("states a sure judgement plainly, and shows no number", () => {
    expect(urgencyHint(row({ urgency: 4, urgency_p: 0.95 }))).toBe("Looks urgent");
    expect(urgencyHint(row({ urgency: 3, urgency_p: 0.93 }))).toBe("Looks like it needs attention soon");
    expect(kindHint(row({ category: "work", category_p: 0.95 }))).toBe("Looks like work. Click to change it.");
    expect(replyHint(row({ reply: 0.97 }))).toBe("Looks like it wants a reply");
    expect(kindHint(row({ category: "work", category_p: 0.95 }))).not.toMatch(/\d\.\d/);
  });

  it("says a kind the person set is theirs, and a judged one is a judgement", () => {
    expect(kindHint(row({ category: "work", category_p: 0.87 }))).toBe("Probably work. Click to change it.");
    expect(kindHint(row({ category: "work", retagged: true }))).toBe("Set by you. Click to change it.");
    expect(replyHint(row({ reply: 0.81 }))).toBe("Probably wants a reply");
  });

  it("writes the time by hand: the hour today, the day before, the year when it is not this one", () => {
    const now = new Date(2026, 9, 23, 15, 0).getTime();
    expect(whenText(new Date(2026, 9, 23, 8, 5).getTime(), now)).toBe("08:05");
    expect(whenText(new Date(2026, 9, 22, 23, 59).getTime(), now)).toBe("22 Oct");
    expect(whenText(new Date(2025, 11, 31, 10, 0).getTime(), now)).toBe("31 Dec 2025");
    expect(whenText(null, now)).toBe("");
  });

  it("says when I last looked, how often, and that nothing leaves this computer", () => {
    const now = new Date(2026, 9, 23, 15, 0).getTime();
    const last = { at: new Date(2026, 9, 23, 9, 15).getTime(), fetched: 3, triaged: 3, errors: [] };
    expect(statusLine(last, 15, now)).toBe("Fetched 09:15 · every 15 minutes · nothing leaves this computer");
    expect(statusLine(null, 15, now)).toBe("Not fetched yet · every 15 minutes · nothing leaves this computer");
  });
});
