/**
 * `UCM-8`: a time read off the front of what was typed, never more than the
 * scheduler can really do, and never saved.
 */
import { describe, expect, it } from "vitest";
import { parseWhen, proposedDraft, scheduleDraft } from "./when";

describe("parseWhen", () => {
  it("reads every hour and every 6 hours exactly", () => {
    expect(parseWhen("every 1 hour check the build")).toEqual({ cadence: "hourly", note: null, rest: "check the build" });
    expect(parseWhen("every 6 hours sync notes")).toMatchObject({ cadence: "six-hourly", note: null, rest: "sync notes" });
    expect(parseWhen("hourly ping")).toMatchObject({ cadence: "hourly", rest: "ping" });
  });

  it("reads 60 and 360 minutes as the hours they are", () => {
    expect(parseWhen("every 60 minutes x")).toMatchObject({ cadence: "hourly", rest: "x" });
    expect(parseWhen("every 360 min x")).toMatchObject({ cadence: "six-hourly" });
  });

  it("leaves the field empty for a rhythm it cannot run, and says why", () => {
    const fast = parseWhen("every 5 minutes poll");
    expect(fast).toMatchObject({ cadence: null, rest: "poll" });
    expect(fast?.note).toContain("every hour");
    expect(parseWhen("every 2 hours poll")?.cadence).toBeNull();
  });

  it("reads daily, with a time it cannot keep said out loud", () => {
    expect(parseWhen("daily summarise my mail")).toEqual({ cadence: "daily", note: null, rest: "summarise my mail" });
    const timed = parseWhen("daily at 09:30 summarise my mail");
    expect(timed).toMatchObject({ cadence: "daily", rest: "summarise my mail" });
    expect(timed?.note).toContain("set time");
    expect(parseWhen("every day at 9pm tidy")?.note).toContain("set time");
  });

  it("reads weekdays as every day, and says it cannot skip weekends", () => {
    const w = parseWhen("every weekday at 9 summarise my mail");
    expect(w).toMatchObject({ cadence: "daily", rest: "summarise my mail" });
    expect(w?.note).toContain("weekends");
    expect(w?.note).toContain("set time");
  });

  it("reads a named day as weekly, and says it cannot pick the day", () => {
    const m = parseWhen("every monday send the report");
    expect(m).toMatchObject({ cadence: "weekly", rest: "send the report" });
    expect(m?.note).toContain("pick the day");
    expect(parseWhen("weekly review")).toEqual({ cadence: "weekly", note: null, rest: "review" });
  });

  it("will not run something once, and says so", () => {
    const t = parseWhen("tomorrow at 8 call mum");
    expect(t).toMatchObject({ cadence: null, rest: "call mum" });
    expect(t?.note).toContain("once");
  });

  it("takes a leading 'to' or punctuation off the task", () => {
    expect(parseWhen("daily, to back up")?.rest).toBe("back up");
    expect(parseWhen("daily: back up")?.rest).toBe("back up");
  });

  it("is not fooled by a number that is not a time", () => {
    // No "at": the 5 belongs to the task.
    expect(parseWhen("every monday 5 reports")).toMatchObject({ cadence: "weekly", note: expect.stringContaining("pick the day"), rest: "5 reports" });
    expect(parseWhen("every monday 5 reports")?.note).not.toContain("set time");
    // Not a real clock time: left out, and said.
    expect(parseWhen("daily at 99 x")).toMatchObject({ cadence: "daily", rest: "x" });
    expect(parseWhen("daily at 25 x")?.note).toContain("couldn't read that time");
  });

  it("says an hourly task has no time, rather than dropping it", () => {
    const h = parseWhen("hourly at 9 ping");
    expect(h).toMatchObject({ cadence: "hourly", rest: "ping" });
    expect(h?.note).toContain("no set time");
  });

  it("is not a time when the words do not start with one", () => {
    expect(parseWhen("summarise my mail every monday")).toBeNull();
    expect(parseWhen("every other day")).toBeNull();
    expect(parseWhen("")).toBeNull();
  });
});

describe("the draft it makes", () => {
  it("fills the field and the task from one line", () => {
    expect(scheduleDraft("Chat", "c1", "every 6 hours sync")).toEqual({
      name: "Chat",
      prompt: "sync",
      conversationId: "c1",
      cadence: "six-hourly",
    });
  });

  it("leaves the field empty when there is no time in the words", () => {
    expect(scheduleDraft("Chat", "c1", "summarise my mail")).toEqual({
      name: "Chat",
      prompt: "summarise my mail",
      conversationId: "c1",
      cadence: null,
    });
  });

  it("keeps what I proposed on screen when it cannot be run as said", () => {
    const d = proposedDraft("Chat", "c1", "every 5 minutes", "poll the site");
    expect(d.cadence).toBeNull();
    expect(d.prompt).toBe("poll the site");
    expect(d.whenNote).toContain("every 5 minutes");
    const free = proposedDraft("Chat", "c1", "whenever you like", "poll");
    expect(free.whenNote).toContain("whenever you like");
  });
});
