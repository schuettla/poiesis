/**
 * `GOL-T1`: the goal loop stops on met, on Stop, on an aborted or failed run, on
 * exhaustion, and when the user leaves the chat. It is driven only by the end of
 * the lead's own turn, through a host the test controls.
 */
import { describe, expect, it } from "vitest";
import type { GoalCheck } from "./api";
import {
  afterRun,
  continuingNote,
  continuingTurn,
  newGoal,
  openingTurn,
  parseGoalArgs,
  type Goal,
  type GoalHost,
} from "./goal";

const NOT_YET: GoalCheck = { met: false, evidence: "", next: "2 tests still fail" };
const MET: GoalCheck = { met: true, evidence: "npm test exited 0", next: "" };

function harness(checks: (GoalCheck | Error)[], opts: { open?: boolean } = {}) {
  const goals: Record<string, Goal> = { c1: newGoal("make the tests pass", "npm test exits 0") };
  const sent: string[] = [];
  const notes: string[] = [];
  let asked = 0;
  let open = opts.open ?? true;
  const host: GoalHost = {
    get: (id) => goals[id],
    isOpen: () => open,
    patch: (id, p) => {
      goals[id] = { ...goals[id], ...p };
    },
    check: async () => {
      const next = checks[asked++];
      if (next instanceof Error) throw next;
      return next;
    },
    note: (_id, text) => notes.push(text),
    send: async (_id, text) => {
      sent.push(text);
    },
  };
  return {
    host,
    goals,
    sent,
    notes,
    asked: () => asked,
    leave: () => {
      open = false;
    },
  };
}

describe("parseGoalArgs", () => {
  it("splits the goal from its condition", () => {
    expect(parseGoalArgs("make the tests pass until npm test exits 0")).toEqual({
      text: "make the tests pass",
      until: "npm test exits 0",
    });
  });

  it("uses the goal as its own condition when there is none", () => {
    expect(parseGoalArgs("clean up the docs folder")).toEqual({
      text: "clean up the docs folder",
      until: "clean up the docs folder",
    });
  });

  it("reads stop, and nothing, and a dangling until", () => {
    expect(parseGoalArgs("stop")).toBe("stop");
    expect(parseGoalArgs("  ")).toBeNull();
    expect(parseGoalArgs("until it works")).toEqual({ text: "until it works", until: "until it works" });
    expect(parseGoalArgs("fix it until")).toEqual({ text: "fix it until", until: "fix it until" });
  });

  it("splits at the first until, so the condition can say until too", () => {
    expect(parseGoalArgs("wait until noon until the log says ok")).toEqual({
      text: "wait",
      until: "noon until the log says ok",
    });
  });
});

describe("what is said each round", () => {
  it("the first turn states the goal and when it is done", () => {
    const g = newGoal("make the tests pass", "npm test exits 0");
    expect(openingTurn(g)).toContain("Goal: make the tests pass.");
    expect(openingTurn(g)).toContain("npm test exits 0");
    // No condition of its own: not said twice.
    const same = newGoal("tidy the docs", "tidy the docs");
    expect(openingTurn(same)).not.toContain("count it as done");
  });

  it("a later turn carries what the last check said is left", () => {
    const g = newGoal("make the tests pass", "npm test exits 0");
    expect(continuingTurn(g, NOT_YET)).toBe("Keep going toward our goal: make the tests pass. Last check: 2 tests still fail");
    expect(continuingNote({ ...g, round: 2 })).toBe("Continuing toward our goal (round 2 of 5)");
  });
});

describe("the loop", () => {
  it("is met: ends, keeps the evidence, sends nothing more", async () => {
    const h = harness([MET]);
    await afterRun(h.host, "c1", {});
    expect(h.goals.c1).toMatchObject({ status: "met", checking: false, lastCheck: MET });
    expect(h.sent).toEqual([]);
  });

  it("is not met with rounds left: notes it, then sends the next round", async () => {
    const h = harness([NOT_YET]);
    await afterRun(h.host, "c1", {});
    expect(h.goals.c1).toMatchObject({ status: "active", round: 2, lastCheck: NOT_YET, checking: false });
    expect(h.notes).toEqual(["Continuing toward our goal (round 2 of 5)"]);
    expect(h.sent).toEqual(["Keep going toward our goal: make the tests pass. Last check: 2 tests still fail"]);
  });

  it("a run that ran out of steps is still judged", async () => {
    const h = harness([MET]);
    await afterRun(h.host, "c1", { stopReason: "max_steps" });
    expect(h.goals.c1.status).toBe("met");
  });

  it("runs five rounds at most, then ends exhausted and says so", async () => {
    const h = harness(Array(5).fill(NOT_YET));
    for (let i = 0; i < 5; i++) await afterRun(h.host, "c1", {});
    expect(h.sent).toHaveLength(4);
    expect(h.goals.c1).toMatchObject({ status: "exhausted", round: 5 });
    expect(h.notes[h.notes.length - 1]).toBe("I stopped after 5 rounds. The goal is not met yet: 2 tests still fail");
    // And a sixth run end does nothing: the goal is over.
    await afterRun(h.host, "c1", {});
    expect(h.asked()).toBe(5);
  });

  it("a run the user stopped ends the goal without a check", async () => {
    const h = harness([MET]);
    await afterRun(h.host, "c1", { stopReason: "aborted" });
    expect(h.goals.c1.status).toBe("stopped");
    expect(h.asked()).toBe(0);
    expect(h.sent).toEqual([]);
  });

  it("a failed or timed-out run ends it too", async () => {
    for (const stopReason of ["error", "timeout"] as const) {
      const h = harness([MET]);
      await afterRun(h.host, "c1", { stopReason });
      expect(h.goals.c1.status).toBe("stopped");
      expect(h.asked()).toBe(0);
    }
  });

  it("a goal that is already over is left alone", async () => {
    for (const status of ["met", "stopped", "exhausted"] as const) {
      const h = harness([MET]);
      h.goals.c1.status = status;
      await afterRun(h.host, "c1", {});
      expect(h.asked()).toBe(0);
      expect(h.sent).toEqual([]);
    }
  });

  it("a chat with no goal is not touched at all", async () => {
    const h = harness([MET]);
    delete h.goals.c1;
    await afterRun(h.host, "c1", {});
    expect(h.asked()).toBe(0);
  });

  it("waits while a plan is waiting for a yes, and does not judge work not done", async () => {
    const h = harness([MET]);
    await afterRun(h.host, "c1", { awaitingApproval: true });
    expect(h.goals.c1).toMatchObject({ status: "active", round: 1 });
    expect(h.asked()).toBe(0);
  });

  it("a check that cannot run stops the goal, and says it did not guess", async () => {
    const h = harness([new Error("no model")]);
    await afterRun(h.host, "c1", {});
    expect(h.goals.c1).toMatchObject({ status: "stopped", checking: false });
    expect(h.notes[0]).toContain("couldn't check");
    expect(h.sent).toEqual([]);
  });

  it("Stop goal while the check runs wins over its answer", async () => {
    const h = harness([NOT_YET]);
    const slow = h.host.check;
    h.host.check = async (id, goal) => {
      const answer = await slow(id, goal);
      h.goals.c1 = { ...h.goals.c1, status: "stopped" };
      return answer;
    };
    await afterRun(h.host, "c1", {});
    expect(h.goals.c1).toMatchObject({ status: "stopped", checking: false });
    expect(h.sent).toEqual([]);
  });

  it("leaving the chat ends it, before and during the check", async () => {
    const before = harness([MET]);
    before.leave();
    await afterRun(before.host, "c1", {});
    expect(before.goals.c1.status).toBe("stopped");
    expect(before.asked()).toBe(0);

    const during = harness([NOT_YET]);
    const slow = during.host.check;
    during.host.check = async (id, goal) => {
      const answer = await slow(id, goal);
      during.leave();
      return answer;
    };
    await afterRun(during.host, "c1", {});
    expect(during.goals.c1.status).toBe("stopped");
    expect(during.sent).toEqual([]);
  });

  it("two run ends at once do not check twice", async () => {
    const h = harness([NOT_YET, NOT_YET]);
    const slow = h.host.check;
    h.host.check = async (id, goal) => {
      await Promise.resolve();
      return slow(id, goal);
    };
    await Promise.all([afterRun(h.host, "c1", {}), afterRun(h.host, "c1", {})]);
    expect(h.asked()).toBe(1);
  });
});
