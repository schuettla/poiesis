/** `FIX-4`: the Self narrative reads as a sentence. */
import { describe, expect, it } from "vitest";
import { narrative } from "./Self";

describe("narrative", () => {
  it("says what I can follow without a stray word", () => {
    const text = narrative(3, 4, 2, 5);
    expect(text).toContain("and I can follow 5 skills.");
    expect(text).not.toContain("I keep");
  });

  it("keeps singulars right", () => {
    const text = narrative(1, 1, 1, 1);
    expect(text).toContain("1 day.");
    expect(text).toContain("1 thing about you");
    expect(text).toContain("1 lesson from");
    expect(text).toContain("1 skill.");
  });

  it("introduces itself when there is nothing yet", () => {
    expect(narrative(0, 0, 0, 0)).toMatch(/^I'm new here/);
  });
});
