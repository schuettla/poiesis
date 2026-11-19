import { describe, expect, it } from "vitest";
import { roleProblem, SHAPES } from "./teamChart";
import { presetMembers, TEAM_PRESETS } from "./teamPresets";

describe("team presets", () => {
  it("have roles the team file takes, a shape there is, and a job for everyone", () => {
    for (const p of TEAM_PRESETS) {
      expect(SHAPES.map((s) => s.id)).toContain(p.shape);
      p.members.forEach((m, i) => {
        expect(roleProblem(m.role, p.members.slice(0, i).map((x) => x.role)), `${p.name}: ${m.role}`).toBeNull();
        expect(m.job.trim()).not.toBe("");
      });
    }
  });

  it("are played by the persona of the name they give when there is one, by a helper otherwise", () => {
    const build = TEAM_PRESETS.find((p) => p.name === "Build and review")!;
    expect(presetMembers(build, []).map((m) => m.persona)).toEqual(["helper", "helper", "helper"]);
    expect(presetMembers(build, [{ id: "b1", name: "Builder" }]).map((m) => m.persona)).toEqual(["persona:b1", "helper", "helper"]);
  });
});
