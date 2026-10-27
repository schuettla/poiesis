import { describe, expect, it } from "vitest";
import { lastTimeLine, lastTimeNote, shortDate } from "./lastTime";

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

  it("names the conversation, the digest and the skill for the model", () => {
    const note = lastTimeNote({
      conversationId: "c-77",
      at: NOON_12_OCT,
      digest: "I made the expense summary per category.  ",
      skill: "expense-report",
    });
    expect(note).toContain("conversation id c-77");
    expect(note).toContain("I made the expense summary per category.");
    expect(note).toContain('The skill I used was "expense-report".');
    expect(note).toContain("unless this message says otherwise");
    expect(note.startsWith("\n\n[Context:")).toBe(true);
    expect(note.endsWith("]")).toBe(true);
  });

  it("leaves the skill sentence out when there was none", () => {
    const note = lastTimeNote({ conversationId: "c-1", at: NOON_12_OCT, digest: "I did it.", skill: null });
    expect(note).not.toContain("skill");
  });
});
