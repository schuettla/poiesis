/**
 * `TEAM-14`: what a team is doing, read from run rows and nothing else. Nothing happened, nothing
 * is said; every state in the chart is also a sentence.
 */
import { describe, expect, it } from "vitest";
import type { PlanView, TeamPost } from "./api";
import { doingWords, itemsOf, nowSentence, pairKey, postLine, postMembers, postsOf, postWidth, readRun, runLabel, taskPosts, taskRuns, whoIs, workingRoles } from "./teamLive";
import type { SubRun } from "./types";

const run = (id: string, member: string | undefined, task: string, over: Partial<SubRun> = {}): SubRun => ({
  runId: id,
  conversationId: `child-${id}`,
  parentConversationId: "c1",
  agent: member ?? "general",
  ...(member ? { member } : {}),
  task,
  status: "running",
  steps: [],
  text: "",
  startedAt: 5000,
  ...over,
});

let postAt = 1000;
const post = (from: string, to: string, kind: TeamPost["kind"], text: string, over: Partial<TeamPost> = {}): TeamPost => ({
  id: `p${postAt}`,
  conversation_id: "c1",
  run_id: null,
  from_member: from,
  to_member: to,
  kind,
  item: null,
  reply_to: null,
  text,
  created_at: postAt++,
  delivered_at: null,
  ...over,
});

const team = {
  coordinator: "poiesis",
  members: [
    { role: "api", persona: "persona:p1", job: "", owns: [] },
    { role: "docs", persona: "helper", job: "", owns: [] },
  ],
};
const personas = [{ id: "p1", name: "Atlas", look: null }];

describe("reading a run", () => {
  it("tells a hand-over, a review and a send-back by the words the loop wrote", () => {
    expect(readRun(run("a", "api", "Build the endpoint", { item: 2 }))).toEqual({ kind: "hand-over", item: 2 });
    expect(readRun(run("b", "verifier", "Review item 3 by api: Build it\n\nreport"))).toEqual({ kind: "review", item: 3, other: "api" });
    expect(readRun(run("c", "api", "Sent back by verifier on item 3: no test"))).toEqual({ kind: "send-back", item: 3, other: "verifier" });
  });
});

describe("drawing only from what happened", () => {
  it("says nothing before anything happened", () => {
    expect(postsOf([])).toEqual({});
    expect(workingRoles([])).toEqual([]);
    expect(itemsOf(undefined, ["api"])).toEqual({});
    expect(nowSentence({ runs: [], live: false, asking: false, stopped: false, coordinator: "Poiesis" })).toBe("");
  });

  it("counts each post along the line it went, whichever way", () => {
    const posts = [
      post("coordinator", "api", "hand-over", "Build it", { item: 1 }),
      post("api", "coordinator", "result", "Built it", { item: 1 }),
      post("api", "verifier", "review", "Built it", { item: 1 }),
      post("verifier", "api", "sent-back", "No test", { item: 1 }),
      post("api", "verifier", "ask", "Which test?"),
    ];
    const counts = postsOf(posts);
    expect(counts[pairKey("coordinator", "api")]).toBe(2);
    expect(counts[pairKey("verifier", "api")]).toBe(3);
    expect(Object.keys(counts).sort()).toEqual(["api|coordinator", "api|verifier"]);
    expect(postsOf([])).toEqual({});
  });

  it("takes the posts of the task on screen: from its run, or from its earliest run after a restart", () => {
    const old = post("coordinator", "api", "hand-over", "Old", { created_at: 100 });
    const fresh = post("coordinator", "api", "hand-over", "New", { created_at: 9000 });
    expect(taskPosts([old, fresh], { startedAt: 8000 }, [])).toEqual([fresh]);
    expect(taskPosts([old, fresh], undefined, [run("n", "api", "y", { startedAt: 8500 })])).toEqual([fresh]);
    // A question is written a moment before the member it wakes starts.
    expect(taskPosts([post("coordinator", "api", "ask", "Which?", { created_at: 8400 })], undefined, [run("n", "api", "y", { startedAt: 8500 })])).toHaveLength(1);
    expect(taskPosts([old, fresh], undefined, [])).toEqual([]);
  });

  it("says a post in a line, with the item and what was said, clipped", () => {
    expect(postLine(post("api", "verifier", "review", "Built it.", { item: 3 }))).toBe("api to verifier, review, item 3: “Built it.”");
    expect(postLine(post("ui", "api", "ask", "x".repeat(200)), 20)).toBe(`ui to api, question: “${"x".repeat(19)}…”`);
    expect(postLine(post("coordinator", "you", "report", "  "))).toBe("coordinator to you, report");
    expect(postMembers([post("api", "coordinator", "note", "a"), post("coordinator", "ui", "note", "b")]).sort()).toEqual(["api", "coordinator", "ui"]);
  });

  it("a queued member is not working, and one that ended is not either", () => {
    const rows = [run("a", "api", "x", { waiting: true }), run("b", "docs", "y", { status: "done" }), run("c", undefined, "z")];
    expect(workingRoles(rows)).toEqual([]);
    expect(workingRoles([...rows, run("d", "docs", "y2")])).toEqual(["docs"]);
  });

  it("puts each item with the member that has it", () => {
    const plan: PlanView = {
      revisions: 0,
      items: [
        { text: "Schema", status: "done", by: "api" },
        { text: "Export", status: "doing", by: "api", review: { state: "checking" } },
        { text: "Docs", status: "todo" },
        { text: "Mail", status: "doing", by: "Claude Code" },
      ],
    };
    const got = itemsOf(plan, ["api", "docs"]);
    expect(got.api.map((i) => [i.n, i.state, i.word])).toEqual([
      [1, "done", "done"],
      [2, "checking", "being checked"],
    ]);
    expect(got.docs).toBeUndefined();
  });

  it("widens a line with posts, and never past six", () => {
    expect(postWidth(1)).toBeLessThan(postWidth(3));
    expect(postWidth(6)).toBe(postWidth(40));
  });
});

describe("the Now sentence", () => {
  const f = { live: true, asking: false, stopped: false, coordinator: "Mira" };
  it("names who looks at what, in the words of the plan", () => {
    const rows = [run("b", "verifier", "Review item 3 by api: x")];
    expect(doingWords(rows[0])).toBe("verifier is looking at item 3 from api");
    expect(nowSentence({ ...f, runs: rows })).toBe("Now: verifier is looking at item 3 from api.");
  });

  it("says the coordinator is working when no member is, waiting and a stopped line last", () => {
    expect(nowSentence({ ...f, runs: [] })).toBe("Now: Mira is working.");
    expect(nowSentence({ ...f, runs: [], asking: true, stopped: true })).toBe("Now: Mira is working; the line is stopped; waiting for you.");
    expect(nowSentence({ ...f, live: false, runs: [] })).toBe("");
  });

  it("does not list more than three, and has no numbers of members", () => {
    const rows = ["a", "b", "c", "d"].map((r, i) => run(r, r, "t", { startedAt: i }));
    const s = nowSentence({ ...f, runs: rows });
    expect(s).toBe("Now: a is working; b is working; c is working; others are working.");
  });
});

describe("which task", () => {
  it("is the run the window knows, else the newest turn that handed work out", () => {
    const old = run("old", "api", "x", { startedAt: 100 });
    const fresh = run("new", "api", "y", { startedAt: 9000 });
    expect(taskRuns([old, fresh], { startedAt: 8000 }, [])).toEqual([fresh]);
    expect(taskRuns([old, fresh], undefined, [{ subRunIds: ["old"] }, { subRunIds: ["new"] }, {}])).toEqual([fresh]);
    expect(taskRuns([old, fresh], undefined, [])).toEqual([]);
  });
});

describe("names", () => {
  it("is role and persona for a member, the role alone for a helper, the agent type for anyone else", () => {
    expect(runLabel(run("a", "api", "x"), team, personas)).toBe("api · Atlas");
    expect(runLabel(run("b", "docs", "x"), team, personas)).toBe("docs");
    expect(runLabel(run("c", undefined, "x", { agent: "general" }), team, personas)).toBe("general");
    expect(runLabel(run("d", "api", "x", { agent: "api" }), null, personas)).toBe("api");
  });

  it("knows Poiesis, a persona, a persona that is gone, and a helper", () => {
    expect(whoIs(team, "coordinator", personas)).toEqual({ kind: "poiesis", name: "Poiesis" });
    expect(whoIs(team, "api", personas).name).toBe("Atlas");
    expect(whoIs(team, "docs", personas)).toEqual({ kind: "helper", name: null });
    expect(whoIs({ ...team, members: [{ role: "api", persona: "persona:p9", job: "", owns: [] }] }, "api", personas).name).toBe("a persona that is gone");
  });
});
