/**
 * `TEAM-14`: under reduced motion only the "Now:" sentence changes. Every movement the Coordination
 * chart has (the orb of a working member, the glow where You are, the stopped line) lives behind
 * `prefers-reduced-motion: no-preference`, so a person who asked for none gets a still drawing and
 * the sentence. The stylesheet is read from disk: Vite hands tests an empty string for `.css?raw`,
 * and the project has no node typings, so the module is loaded without them.
 */
import { beforeAll, describe, expect, it } from "vitest";

let css = "";
beforeAll(async () => {
  // @ts-expect-error no @types/node in this project
  const fs: { readFileSync(path: string, enc: string): string } = await import(/* @vite-ignore */ "node:fs");
  css = fs.readFileSync("src/components/Workbench/OrgDiagram.css", "utf8");
});

describe("the Coordination chart's motion", () => {
  it("is all behind no-preference", () => {
    expect(css.length).toBeGreaterThan(1000);
    // Without the blocks that only apply to people who allow motion, and the keyframes they use.
    let still = css.replace(/@keyframes[^{]+\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, "");
    still = still.replace(/@media \(prefers-reduced-motion: no-preference\) \{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, "");
    expect(still).not.toMatch(/animation|transition/);
  });

  it("gives a working member's orb its movement there, and the sentence is not one of them", () => {
    expect(css).toMatch(/prefers-reduced-motion: no-preference\) \{[^@]*\.org-node\.working \.org-orb/);
    expect(css).not.toMatch(/\.org-sentence[^{]*\{[^}]*animation/);
  });
});
