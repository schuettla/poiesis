import { describe, expect, it } from "vitest";
import { NoticePacer, noticeFor } from "./notices";

describe("noticeFor (VTN-5)", () => {
  it("picks a line from the kind of work", () => {
    expect(noticeFor("searched the web", "en")).toBe("Let me look that up.");
    expect(noticeFor("worked out", "en")).toBe("Let me work that out.");
    expect(noticeFor("wrote a file", "en")).toBe("I'll write that down.");
  });

  it("speaks German to a German speaker", () => {
    expect(noticeFor("searched the web", "de")).toBe("Ich schau kurz nach.");
    expect(noticeFor("ran a script", "de-DE")).toBe("Ich rechne das kurz durch.");
    expect(noticeFor("wrote a file", "de")).toBe("Ich schreibe das auf.");
  });

  it("says nothing for plain work or an unknown step", () => {
    expect(noticeFor("remembered", "en")).toBeNull();
    expect(noticeFor("did something", "en")).toBeNull();
  });

  it("falls back to English when the language is unknown", () => {
    expect(noticeFor("searched", null)).toBe("Let me look that up.");
    expect(noticeFor("searched", "fr")).toBe("Let me look that up.");
  });
});

describe("NoticePacer", () => {
  it("waits for a quiet moment", () => {
    const p = new NoticePacer();
    expect(p.due(1000, 600)).toBe(false);
    expect(p.due(2100, 600)).toBe(true);
  });

  it("allows one notice per ten seconds", () => {
    const p = new NoticePacer();
    expect(p.due(5000, 0)).toBe(true);
    p.said(5000);
    expect(p.due(9000, 0)).toBe(false);
    expect(p.due(15_000, 0)).toBe(true);
  });
});
