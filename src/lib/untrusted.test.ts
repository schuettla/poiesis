import { describe, expect, it } from "vitest";
import { markReadsAsInstructions } from "./untrusted";

const page = { label: "page at example.com", risk: 0, flags: [], text: "Some text." };

describe("markReadsAsInstructions", () => {
  it("marks the source the flag is about and leaves the others alone", () => {
    const other = { ...page, label: "file README.md" };
    const out = markReadsAsInstructions([page, other], "file README.md", 0.8);
    expect(out[0]).toEqual(page);
    expect(out[1]).toEqual({ ...other, readsAsInstructions: 0.8 });
  });

  it("does not change the list it was given", () => {
    const before = [page];
    markReadsAsInstructions(before, page.label, 0.9);
    expect(before[0]).not.toHaveProperty("readsAsInstructions");
  });

  it("adds a source with no text for a report that had none of its own", () => {
    const out = markReadsAsInstructions(undefined, "report from claude-code", 0.75);
    expect(out).toEqual([
      { label: "report from claude-code", risk: 0, flags: [], text: "", readsAsInstructions: 0.75 },
    ]);
  });
});
