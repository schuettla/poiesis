import { describe, expect, it } from "vitest";
import { composeSystemPrompt, SKILL_ENTRY_CAP, SKILLS_BLOCK_CAP } from "./store";
import type { SkillView } from "./api";

/** `SKL-2`'s stage-1 disclosure: a minimal, otherwise-default skill so each
 * test only has to spell out the field it cares about. */
function skill(overrides: Partial<SkillView>): SkillView {
  return {
    name: "a-skill",
    description: "does a thing",
    when_to_use: null,
    source: "app",
    dir: "/skills/a-skill",
    enabled: true,
    unsupported: [],
    used: 0,
    rough: 0,
    risk: 0,
    risk_flags: [],
    ...overrides,
  };
}

const BASE = "You are Poiesis.";

function withSkills(skills: SkillView[], toolsEnabled = true) {
  return composeSystemPrompt(BASE, {
    conv: undefined,
    sessionState: undefined,
    toolsEnabled,
    skills,
  });
}

describe("composeSystemPrompt — skills block (SKL-2)", () => {
  it("omits the block entirely when tools are disabled, even with enabled skills", () => {
    const prompt = withSkills([skill({ name: "pdf-forms" })], false);
    expect(prompt).not.toContain("Skills available");
    expect(prompt).not.toContain("pdf-forms");
  });

  it("omits the block when there are no enabled skills", () => {
    const prompt = withSkills([skill({ name: "off", enabled: false })]);
    expect(prompt).not.toContain("Skills available");
  });

  it("lists enabled skills by name and description, excluding disabled ones", () => {
    const prompt = withSkills([
      skill({ name: "pdf-forms", description: "Fill and flatten PDF forms.", when_to_use: "a form to complete" }),
      skill({ name: "off-skill", enabled: false }),
    ]);
    expect(prompt).toContain("Skills available (read one with the `skill` tool before doing the work it covers):");
    expect(prompt).toContain("- pdf-forms: Fill and flatten PDF forms. — a form to complete");
    expect(prompt).not.toContain("off-skill");
  });

  it("truncates one entry's description+when_to_use combined at SKILL_ENTRY_CAP chars", () => {
    const long = "x".repeat(SKILL_ENTRY_CAP + 200);
    const prompt = withSkills([skill({ name: "verbose", description: long, when_to_use: null })]);
    const line = prompt.split("\n").find((l) => l.startsWith("- verbose:"));
    expect(line).toBeDefined();
    // "- verbose: " prefix + the clipped description + the "…" marker.
    const body = line!.slice("- verbose: ".length);
    expect(body.endsWith("…")).toBe(true);
    expect(body.length).toBe(SKILL_ENTRY_CAP + 1); // +1 for the ellipsis char
    expect(body.slice(0, -1)).toBe(long.slice(0, SKILL_ENTRY_CAP));
  });

  it("does not truncate a description at or under the cap", () => {
    const exact = "x".repeat(SKILL_ENTRY_CAP);
    const prompt = withSkills([skill({ name: "exact", description: exact, when_to_use: null })]);
    expect(prompt).toContain(`- exact: ${exact}`);
    expect(prompt).not.toContain(`${exact}…`);
  });

  it("stops adding entries once SKILLS_BLOCK_CAP is reached and appends a (+n more) line", () => {
    // Each entry's description is long enough that only a few fit under the
    // 4000-char block cap, guaranteeing at least one gets dropped.
    const skills = Array.from({ length: 10 }, (_, i) => skill({ name: `skill-${i}`, description: "y".repeat(900) }));
    const prompt = withSkills(skills);

    // Isolated to just the skills block (up to the next blank-line-separated
    // section) so the cap check below isn't diluted by the rest of the prompt.
    const afterHeader = prompt.slice(prompt.indexOf("Skills available"));
    const skillsBlockText = afterHeader.slice(0, afterHeader.indexOf("\n\n"));
    expect(skillsBlockText.length).toBeLessThanOrEqual(SKILLS_BLOCK_CAP + 40); // + the "(+n more)" line

    const lines = skillsBlockText.split("\n");
    const entryLines = lines.filter((l) => l.startsWith("- skill-"));
    const moreLine = lines.find((l) => /^\(\+\d+ more\)$/.test(l));

    expect(entryLines.length).toBeLessThan(skills.length);
    expect(moreLine).toBeDefined();
    const remaining = Number(moreLine!.match(/\d+/)![0]);
    expect(remaining).toBe(skills.length - entryLines.length);
  });

  it("adds no (+n more) line when every enabled skill fits", () => {
    const prompt = withSkills([skill({ name: "one" }), skill({ name: "two" })]);
    expect(prompt).not.toMatch(/\(\+\d+ more\)/);
  });

  it("still leads with the base prompt when there are no skills at all", () => {
    const prompt = withSkills([]);
    expect(prompt.startsWith(BASE)).toBe(true);
    expect(prompt).not.toContain("Skills available");
  });
});
