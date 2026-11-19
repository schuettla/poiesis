import { describe, expect, it } from "vitest";
import type { TeamLine, TeamMember } from "./api";
import {
  CHART_H,
  CHART_W,
  edgePath,
  isFixedLine,
  layoutTeam,
  lineKindsFor,
  lineWords,
  pairsOf,
  roleFrom,
  roleProblem,
  sameLines,
  teamNamer,
  withMember,
  withNewLines,
  withoutRole,
} from "./teamChart";

const m = (role: string, persona = "helper"): TeamMember => ({ role, persona, job: "", owns: [] });
const l = (kind: TeamLine["kind"], from: string, to: string): TeamLine => ({ kind, from, to });
const FIXED = [l("reports", "coordinator", "you"), l("talks", "you", "coordinator")];
const at = (places: ReturnType<typeof layoutTeam>, key: string) => places.find((p) => p.key === key)!;

describe("the layout of each shape", () => {
  const abc = [m("a"), m("b"), m("c")];

  it("a hub puts the coordinator under You and the members in one row under the coordinator", () => {
    const p = layoutTeam({ shape: "hub", members: abc, lines: [...abc.map((x) => l("hands-over", "coordinator", x.role)), ...FIXED] });
    expect(at(p, "you").y).toBeLessThan(at(p, "coordinator").y);
    expect(at(p, "coordinator").x).toBe(at(p, "you").x);
    const row = abc.map((x) => at(p, x.role));
    expect(new Set(row.map((r) => r.y)).size).toBe(1);
    expect(row[0].y).toBeGreaterThan(at(p, "coordinator").y);
    expect(row.map((r) => r.x)).toEqual([...row.map((r) => r.x)].sort((x, y) => x - y));
  });

  it("a pipeline runs left to right in the order of the members", () => {
    const p = layoutTeam({ shape: "pipeline", members: abc, lines: [] });
    const xs = ["coordinator", "a", "b", "c"].map((k) => at(p, k).x);
    expect(xs).toEqual([...xs].sort((x, y) => x - y));
    expect(new Set(["coordinator", "a", "b", "c"].map((k) => at(p, k).y)).size).toBe(1);
    expect(at(p, "you").y).toBeLessThan(at(p, "coordinator").y);
  });

  it("a review puts the reviewer under what it reviews, and not in the workers' row", () => {
    const lines = [l("hands-over", "coordinator", "a"), l("hands-over", "coordinator", "b"), l("reviews", "c", "a"), l("reviews", "c", "b"), ...FIXED];
    const p = layoutTeam({ shape: "review", members: abc, lines });
    expect(at(p, "c").y).toBeGreaterThan(at(p, "a").y);
    expect(at(p, "a").y).toBe(at(p, "b").y);
    const [a, b] = [at(p, "a").x, at(p, "b").x];
    expect(at(p, "c").x).toBeCloseTo((a + b) / 2, 5);
  });

  it("open puts everyone around one table, with the coordinator on top", () => {
    const p = layoutTeam({ shape: "open", members: abc, lines: [] });
    const ys = ["a", "b", "c"].map((k) => at(p, k).y);
    expect(at(p, "coordinator").y).toBeLessThan(Math.min(...ys));
    expect(at(p, "you").y).toBeLessThan(at(p, "coordinator").y);
    const keys = new Set(p.map((x) => `${Math.round(x.x)},${Math.round(x.y)}`));
    expect(keys.size).toBe(p.length);
  });

  it("every place stays inside the box, for a team with no members and a team with eight", () => {
    const many = Array.from({ length: 8 }, (_, i) => m(`m${i}`));
    for (const shape of ["hub", "pipeline", "review", "open"] as const) {
      for (const members of [[], many]) {
        const lines = members.length ? [l("reviews", "m7", "m0")] : [];
        for (const p of layoutTeam({ shape, members, lines })) {
          expect(p.x - p.r).toBeGreaterThanOrEqual(0);
          expect(p.x + p.r).toBeLessThanOrEqual(CHART_W);
          expect(p.y - p.r).toBeGreaterThanOrEqual(0);
          expect(p.y + p.r + 14).toBeLessThanOrEqual(CHART_H);
        }
      }
    }
  });
});

describe("lines on the chart", () => {
  it("draws one line for two places, with an arrowhead at each end that has a line", () => {
    const pairs = pairsOf([l("hands-over", "coordinator", "a"), l("asks", "a", "coordinator"), l("talks", "you", "coordinator"), l("reports", "coordinator", "you")]);
    expect(pairs.map((p) => p.key).sort()).toEqual(["a|coordinator", "coordinator|you"]);
    const ac = pairs.find((p) => p.key === "a|coordinator")!;
    expect([ac.kind, ac.toA, ac.toB]).toEqual(["hands-over", true, true]);
    expect(ac.lines).toHaveLength(2);
    const yc = pairs.find((p) => p.key === "coordinator|you")!;
    expect(yc.kind).toBe("talks");
  });

  it("is straight, and bows out of the way of a place it would pass through", () => {
    const a = { key: "a", x: 0, y: 0, r: 10, room: 50 };
    const b = { key: "b", x: 200, y: 0, r: 10, room: 50 };
    const between = { key: "c", x: 100, y: 0, r: 10, room: 50 };
    expect(edgePath(a, b, [])).toMatch(/^M[\d. -]+ L/);
    const bowed = edgePath(a, b, [between]);
    expect(bowed).toContain("Q");
    // Down, never up.
    const control = bowed.match(/Q(-?[\d.]+) (-?[\d.]+)/)!;
    expect(Number(control[2])).toBeGreaterThan(0);
  });

  it("offers only the lines that make sense between two places", () => {
    expect(lineKindsFor("you", "coordinator")).toEqual(["talks"]);
    expect(lineKindsFor("api", "you")).toEqual(["reports"]);
    expect(lineKindsFor("api", "ui")).toEqual(["hands-over", "reviews", "asks", "reports"]);
    expect(lineKindsFor("api", "api")).toEqual([]);
  });

  it("says a line in words, with the persona's name for the coordinator", () => {
    const name = teamNamer({ coordinator: "persona:p1" }, [{ id: "p1", name: "Mira" }]);
    expect(lineWords(l("reviews", "verifier", "api"), name)).toBe("verifier reviews api's work");
    expect(lineWords(l("hands-over", "coordinator", "api"), name)).toBe("Mira hands over work to api");
    expect(lineWords(l("talks", "you", "coordinator"), name)).toBe("You can talk with Mira");
    expect(lineWords(l("reports", "coordinator", "you"), name)).toBe("Mira reports to you");
    expect(lineWords(l("asks", "api", "ui"), teamNamer({ coordinator: "poiesis" }, []))).toBe("api may ask ui");
    expect(lineWords(l("reports", "coordinator", "you"), teamNamer({ coordinator: "poiesis" }, []))).toBe("Poiesis reports to you");
  });

  it("knows the two lines nobody can take away", () => {
    expect(FIXED.every(isFixedLine)).toBe(true);
    expect(isFixedLine(l("reports", "api", "you"))).toBe(false);
  });

  it("compares lines whatever their order", () => {
    expect(sameLines([l("asks", "a", "b"), ...FIXED], [...FIXED, l("asks", "a", "b")])).toBe(true);
    expect(sameLines([l("asks", "a", "b")], [l("asks", "b", "a")])).toBe(false);
  });
});

describe("members", () => {
  it("go last, except in Review, where the last one is the reviewer", () => {
    const ab = [m("a"), m("b")];
    expect(withMember("hub", ab, m("x")).map((x) => x.role)).toEqual(["a", "b", "x"]);
    expect(withMember("review", ab, m("x")).map((x) => x.role)).toEqual(["a", "x", "b"]);
    expect(withMember("review", [m("a")], m("x")).map((x) => x.role)).toEqual(["a", "x"]);
  });

  it("a new member brings only its own lines, and what was drawn by hand stays", () => {
    const before = [l("hands-over", "coordinator", "a"), ...FIXED];
    const after = [l("hands-over", "coordinator", "a"), l("hands-over", "coordinator", "b"), ...FIXED];
    const mine = [...before, l("asks", "a", "coordinator")];
    expect(withNewLines(mine, before, after)).toEqual([...mine, l("hands-over", "coordinator", "b")]);
  });

  it("taking one out takes its lines out", () => {
    expect(withoutRole([l("hands-over", "coordinator", "a"), l("reviews", "b", "a"), l("hands-over", "coordinator", "b")], "a")).toEqual([
      l("hands-over", "coordinator", "b"),
    ]);
  });

  it("have role names the saved file accepts", () => {
    expect(roleFrom("Terse Code Reviewer!")).toBe("terse-code-reviewer");
    expect(roleProblem("api", ["ui"])).toBeNull();
    expect(roleProblem("", [])).toBeTruthy();
    expect(roleProblem("you", [])).toContain("taken");
    expect(roleProblem("Api", [])).toContain("small letters");
    expect(roleProblem("ui", ["ui"])).toContain("Two members");
  });
});
