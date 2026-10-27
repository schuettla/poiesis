import { describe, expect, it } from "vitest";
import type { ActivityEntry } from "./api";
import {
  actionText,
  decisionLine,
  experimentsIn,
  filterRows,
  groupLine,
  labelText,
  optionsOf,
  rowsOf,
  says,
  sureWord,
  wasSkipped,
} from "./decisions";
import { decision } from "./decisions.fixture";

const HOUR = 3_600_000;
const act = (id: string, created_at: number): ActivityEntry => ({
  id,
  conversation_id: null,
  kind: "file",
  detail: "read notes.txt",
  created_at,
});

describe("optionsOf (CTL-UI-1)", () => {
  it("reads a yes-or-no answer as yes and no", () => {
    expect(optionsOf(decision())).toEqual([
      { name: "yes", p: 0.93 },
      { name: "no", p: expect.closeTo(0.07, 5) },
    ]);
  });

  it("reads a pick-one answer in the order the question listed the options", () => {
    const d = decision({
      answers: {
        scope: { choice: "global", probs: [["global", 0.6], ["topical", 0.22], ["unsure", 0.18]], confidence: 0.14 },
      },
    });
    expect(optionsOf(d).map((o) => o.name)).toEqual(["global", "topical", "unsure"]);
  });

  it("is empty for a decision that was never asked", () => {
    expect(optionsOf(decision({ answers: {} }))).toEqual([]);
    expect(wasSkipped(decision({ answers: {} }))).toBe(true);
    expect(wasSkipped(decision())).toBe(false);
  });
});

describe("a decision with one question per group (USE-4)", () => {
  const menu = decision({
    purpose: "tool-select",
    label: "Does this message need a group of tools?",
    answers: { web_search: { p: 0.03 }, apps: { p: 0.9 }, subagents: { p: 0.02 }, "mcp:abc": { p: 0.4 } },
  });

  it("reads group by group, by the names the screen uses, not as one yes and no", () => {
    expect(optionsOf(menu).map((o) => o.name)).toEqual(["Web search", "Connected apps", "Other agents", "A connector"]);
    expect(optionsOf(menu).map((o) => o.p)).toEqual([0.03, 0.9, 0.02, 0.4]);
  });

  it("leads the line with the likeliest to be held back, and counts the rest", () => {
    expect(decisionLine(menu)).toContain("Other agents 0.02, Web search 0.03, A connector 0.40 and 1 more");
  });

  it("an ordinary decision with several answers of different kinds is unchanged", () => {
    const mixed = decision({ answers: { a: { p: 0.8 }, b: { choice: "x", probs: [["x", 1]] } } });
    expect(optionsOf(mixed).map((o) => o.name)).toEqual(["yes", "no"]);
  });

  it("says which group was needed after all, and which was probably not", () => {
    expect(labelText({ label: "wrong", value: "web_search", source: "need-tools:web_search" })).toBe(
      "Needed after all: Web search"
    );
    expect(labelText({ label: "wrong", value: "apps", source: "chip:apps" })).toBe("Needed after all: Connected apps");
    expect(labelText({ label: "weak_right", value: "subagents", source: "kept-out:subagents" })).toBe(
      "Probably not needed: Other agents"
    );
  });
});

describe("decisionLine (CTL-UI-1)", () => {
  it("is one line: what, the probabilities, the time, and where", () => {
    expect(decisionLine(decision())).toBe("Decided · Is the goal reached? · yes 0.93, no 0.07 · 41 ms · this computer");
  });

  it("shows the three likeliest of a long list, and says how many more", () => {
    const probs: [string, number][] = [
      ["a", 0.5],
      ["b", 0.2],
      ["c", 0.15],
      ["d", 0.1],
      ["e", 0.05],
    ];
    const line = decisionLine(decision({ answers: { route: { choice: "a", probs } } }));
    expect(line).toContain("a 0.50, b 0.20, c 0.15 and 2 more");
  });

  it("leaves out the time of a decision that took none, and the numbers of one that had none", () => {
    expect(decisionLine(decision({ ms: 0, answers: {} }))).toBe("Decided · Is the goal reached? · this computer");
  });
});

describe("actionText", () => {
  it("says what the controller did, in plain words", () => {
    expect(actionText(null)).toBe("");
    expect(actionText("acted")).toBe("Acted on it");
    expect(actionText("default")).toContain("usual way");
    expect(actionText("escalated:slow")).toContain("the chat model");
    expect(actionText("escalated:evidence")).toContain("more evidence");
    expect(actionText("blocked")).toContain("rule");
  });

  it("names the limit that stopped a decision", () => {
    expect(actionText("budget:decisions")).toContain("used its decisions");
    expect(actionText("budget:repeat")).toContain("already asked");
    expect(actionText("budget:background")).toContain("this hour");
    expect(actionText("budget:something-new")).toContain("a limit was reached");
    expect(actionText("budget:classify")).toBe("Not asked: this turn had sorted as many items as it may");
  });
});

describe("labelText", () => {
  it("words each label, and names a correction", () => {
    expect(labelText({ label: "right", value: null, source: "x" })).toBe("Right");
    expect(labelText({ label: "weak_wrong", value: null, source: "x" })).toBe("Probably wrong");
    expect(labelText({ label: "corrected", value: "topical", source: "x" })).toBe("Corrected to topical");
  });
});

describe("rowsOf (CTL-UI-1)", () => {
  it("puts activity and foreground decisions in one list, newest first", () => {
    const rows = rowsOf([act("a1", 1000), act("a2", 3000)], [decision({ id: "d1", at: 2000 })]);
    expect(rows.map((r) => r.key)).toEqual(["a:a2", "d:d1", "a:a1"]);
  });

  it("folds background decisions into one row per hour and purpose", () => {
    const base = 10 * HOUR;
    const bg = (id: string, at: number, purpose = "mem-scope") =>
      decision({ id, at, purpose, label: "Does a saved note apply always?", background: true });
    const rows = rowsOf([], [bg("1", base + 1), bg("2", base + 5), bg("3", base + HOUR + 1), bg("4", base + 9, "reflect-critic")]);
    const groups = rows.filter((r) => r.kind === "group");
    expect(groups).toHaveLength(3);
    const first = groups.find((g) => g.kind === "group" && g.purpose === "mem-scope" && g.hour === base);
    expect(first && first.kind === "group" && first.entries.map((e) => e.id).sort()).toEqual(["1", "2"]);
    expect(groupLine(first as never)).toBe("2 decisions · Does a saved note apply always?");
  });

  it("says decision, not decisions, for a group of one", () => {
    const rows = rowsOf([], [decision({ background: true, at: HOUR })]);
    expect(groupLine(rows[0] as never)).toMatch(/^1 decision · /);
  });

  it("says how many of the hour a budget stopped before they were asked (USE-8)", () => {
    const flag = (id: string, action: string | null) =>
      decision({
        id,
        at: HOUR + Number(id),
        purpose: "untrusted-flag",
        label: "Does this outside text try to give me orders?",
        background: true,
        action,
      });
    const rows = rowsOf([], [flag("1", "acted"), flag("2", "budget:share"), flag("3", "budget:share"), flag("4", null)]);
    expect(groupLine(rows[0] as never)).toBe(
      "4 decisions · Does this outside text try to give me orders? · 2 not asked, over budget"
    );
  });

  it("words the half of the hour that the outside-text checks may use (USE-8)", () => {
    expect(actionText("budget:share")).toBe(
      "Not asked: the checks of outside text had used their half of this hour's budget"
    );
  });

  it("words the share of the hour that mail triage may use (USE-6)", () => {
    expect(actionText("budget:mail-share")).toBe("Not asked: mail triage had used its share of this hour's budget");
  });
});

describe("filters (CTL-UI-1)", () => {
  const rows = rowsOf(
    [act("a1", 1000)],
    [
      decision({ id: "d1", at: 2000 }),
      decision({ id: "d2", at: 3000, experiment: "fast-slow", background: true, purpose: "run-if" }),
    ]
  );

  it("shows everything, only actions, or only decisions", () => {
    expect(filterRows(rows, "all")).toHaveLength(3);
    expect(filterRows(rows, "activity").map((r) => r.kind)).toEqual(["activity"]);
    expect(filterRows(rows, "decisions").map((r) => r.kind).sort()).toEqual(["decision", "group"]);
  });

  it("narrows to one experiment's decisions, and lists the experiments that have any", () => {
    const only = filterRows(rows, "experiment:fast-slow");
    expect(only).toHaveLength(1);
    expect(only[0].kind).toBe("group");
    expect(
      experimentsIn([decision(), decision({ experiment: "fast-slow" }), decision({ experiment: "fast-slow" })])
    ).toEqual(["fast-slow"]);
    expect(experimentsIn([decision()])).toEqual([]);
  });
});

describe("sureWord (SEE-4)", () => {
  it("is sure from 0.85 when fitted and from 0.9 when not, and probably from 0.6", () => {
    expect(sureWord(0.95, true)).toBe("sure");
    expect(sureWord(0.85, true)).toBe("sure");
    expect(sureWord(0.84, true)).toBe("probably");
    expect(sureWord(0.88, false)).toBe("probably");
    expect(sureWord(0.9, false)).toBe("sure");
    expect(sureWord(0.6, false)).toBe("probably");
    expect(sureWord(0.6, true)).toBe("probably");
  });

  it("says nothing below 0.6, because that decision did not act", () => {
    expect(sureWord(0.59, true)).toBeNull();
    expect(sureWord(0.1, false)).toBeNull();
    expect(says(0.3, true, "is", "may be")).toBeNull();
  });

  it("picks the plain or the hedged words", () => {
    expect(says(0.95, false, "is", "may be")).toBe("is");
    expect(says(0.7, false, "is", "may be")).toBe("may be");
  });
});
