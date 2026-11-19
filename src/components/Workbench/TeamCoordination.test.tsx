/**
 * @vitest-environment jsdom
 *
 * `TEAM-14`: the Coordination view of a session a team leads. The chart is the team's drawing and
 * only what the run's events say is added to it; Members is the helper list grouped by member.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Persona, Team, TeamPost } from "../../lib/api";
import type { RunSummary, SubRun } from "../../lib/types";

const teamGet = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  teamGet: (...a: unknown[]) => teamGet(...a),
}));

import AgentsPanel from "./AgentsPanel";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const CONV = "c1";
let container: HTMLDivElement;
let root: Root;
const openItem = vi.fn();

const line = (kind: Team["lines"][number]["kind"], from: string, to: string) => ({ kind, from, to });
const team: Team = {
  slug: "shop",
  name: "Shop team",
  line: "",
  coordinator: "poiesis",
  style: "lean",
  member_style: "lean",
  shape: "review",
  report: "each-item",
  limits: { at_once: 2, budget: 0, minutes: 30, rounds: 2, ask_cost: true },
  folder: "copies",
  locked: false,
  version: 1,
  members: [
    { role: "api", persona: "persona:p1", job: "", owns: [] },
    { role: "docs", persona: "helper", job: "", owns: [] },
    { role: "verifier", persona: "persona:p2", job: "", owns: [] },
  ],
  lines: [
    line("talks", "you", "coordinator"),
    line("reports", "coordinator", "you"),
    line("hands-over", "coordinator", "api"),
    line("hands-over", "coordinator", "docs"),
    line("reviews", "verifier", "api"),
  ],
};

const sub = (id: string, member: string | undefined, task: string, over: Partial<SubRun> = {}): SubRun => ({
  runId: id,
  conversationId: `child-${id}`,
  parentConversationId: CONV,
  agent: member ?? "general",
  ...(member ? { member } : {}),
  task,
  status: "running",
  steps: [],
  text: "",
  startedAt: 5000,
  ...over,
});

const summary = (over: Partial<RunSummary> = {}): RunSummary => ({
  runId: "r1",
  startedAt: 4000,
  live: true,
  files: [],
  costUsd: null,
  localRun: false,
  style: { preset: "lean", name: "Lean" },
  ...over,
});

let postAt = 5000;
const post = (from: string, to: string, kind: TeamPost["kind"], text: string, over: Partial<TeamPost> = {}): TeamPost => ({
  id: `post-${postAt}`,
  conversation_id: CONV,
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
const postsIn = (...ps: TeamPost[]) => ({ [CONV]: ps });

function put(s: Partial<ReturnType<typeof useAppStore.getState>>) {
  act(() => useAppStore.setState(s as never));
}
const runsOf = (...rs: SubRun[]) => Object.fromEntries(rs.map((r) => [r.runId, r]));

beforeEach(async () => {
  openItem.mockReset();
  teamGet.mockReset().mockResolvedValue({ team, problems: [] });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: CONV,
    conversations: [{ id: CONV, title: "t", updatedAt: 0, messages: [], leader: "team:shop", personaId: null }],
    personas: [
      { id: "p1", name: "Atlas", look: null },
      { id: "p2", name: "Vera", look: null },
    ] as unknown as Persona[],
    presets: [{ slug: "lean", name: "Lean", line: "x", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] }],
    subRuns: {},
    posts: {},
    runSummaries: {},
    pendingQuestion: null,
    openItem,
    loadSubRuns: () => Promise.resolve(),
    loadPosts: () => Promise.resolve(),
  } as never);
  await act(async () => {
    root.render(<AgentsPanel conversationId={CONV} />);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const sentence = () => container.querySelector(".org-sentence")?.textContent;
const nodeOf = (role: string) => [...container.querySelectorAll<SVGGElement>(".org-node")].find((n) => n.querySelector(".org-name")?.textContent === role);
const lineOf = (key: string) => container.querySelector(`.org-line[data-line="${key}"] .org-edge`) as SVGPathElement;
const tab = (label: string) => [...container.querySelectorAll<HTMLButtonElement>("[role=tab]")].find((b) => b.textContent === label)!;

describe("the team's Coordination view", () => {
  it("is the team, its way of working, and the tabs Chart, Tasks and Members, in a box that has a fixed size", () => {
    expect(container.querySelector(".org-head")?.textContent).toBe("Shop teamLean");
    expect([...container.querySelectorAll("[role=tab]")].map((t) => t.textContent)).toEqual(["Chart", "Tasks", "Members"]);
    expect(tab("Chart").getAttribute("aria-selected")).toBe("true");
    // You, the coordinator and the three members, drawn in the shape's places.
    expect(container.querySelectorAll(".org-node")).toHaveLength(5);
    expect(nodeOf("verifier")).toBeDefined();
    expect(container.querySelector("svg.team-svg")).not.toBeNull();
  });

  it("draws nothing it was not told: with no events there is no orb, no wider line, no item, and it rests", () => {
    expect(container.querySelectorAll(".org-orb")).toHaveLength(0);
    expect(container.querySelectorAll(".org-edge[style]")).toHaveLength(0);
    expect(container.querySelectorAll(".org-node.item")).toHaveLength(0);
    expect(sentence()).toBe("Resting");
  });

  it("shows a member that is working with the orb, widens the line it was handed work along, and says so", () => {
    put({
      runSummaries: { [CONV]: summary() },
      subRuns: runsOf(sub("a", "api", "Build it", { item: 2 })),
      posts: postsIn(post("coordinator", "api", "hand-over", "Build it", { item: 2 })),
    });
    expect(nodeOf("api")!.getAttribute("class")).toContain("working");
    expect(nodeOf("api")!.querySelector(".org-orb")).not.toBeNull();
    expect(nodeOf("docs")!.getAttribute("class")).not.toContain("working");
    expect(lineOf("api|coordinator").getAttribute("style")).toMatch(/stroke-width/);
    expect(lineOf("coordinator|docs").getAttribute("style")).toBeNull();
    expect(sentence()).toBe("Now: api is working on item 2.");
  });

  it("follows a sequence of posts: hand-over, result, review, send-back; the lines grow and the sentence follows", () => {
    const width = (key: string) => parseFloat(/stroke-width:\s*([\d.]+)/.exec(lineOf(key).getAttribute("style") ?? "")?.[1] ?? "0");
    const handOver = post("coordinator", "api", "hand-over", "Build it", { item: 1 });
    put({ runSummaries: { [CONV]: summary() }, subRuns: runsOf(sub("a", "api", "Build it", { item: 1 })), posts: postsIn(handOver) });
    const first = width("api|coordinator");
    put({
      subRuns: runsOf(sub("a", "api", "Build it", { item: 1, status: "done" })),
      posts: postsIn(handOver, post("api", "coordinator", "result", "Built it", { item: 1 })),
    });
    expect(width("api|coordinator")).toBeGreaterThan(first);
    expect(width("api|verifier")).toBe(0);

    const review = post("api", "verifier", "review", "Built it", { item: 1 });
    const result = post("api", "coordinator", "result", "Built it", { item: 1 });
    put({
      subRuns: runsOf(sub("a", "api", "Build it", { item: 1, status: "done" }), sub("b", "verifier", "Review item 1 by api: Build it", { startedAt: 6000 })),
      posts: postsIn(handOver, result, review),
    });
    expect(width("api|verifier")).toBeGreaterThan(0);
    expect(nodeOf("verifier")!.getAttribute("class")).toContain("working");
    expect(nodeOf("api")!.getAttribute("class")).not.toContain("working");
    expect(sentence()).toBe("Now: verifier is looking at item 1 from api.");

    const reviewed = width("api|verifier");
    put({
      subRuns: runsOf(
        sub("a", "api", "Build it", { item: 1, status: "done" }),
        sub("b", "verifier", "Review item 1 by api: Build it", { startedAt: 6000, status: "done" }),
        sub("c", "api", "Sent back by verifier on item 1: no test", { startedAt: 7000 })
      ),
      posts: postsIn(handOver, result, review, post("verifier", "api", "sent-back", "no test", { item: 1 })),
    });
    expect(width("api|verifier")).toBeGreaterThan(reviewed);
    expect(sentence()).toBe("Now: api is fixing item 1 after verifier sent it back.");
  });

  it("does not count what an earlier task did", () => {
    put({
      runSummaries: { [CONV]: summary({ startedAt: 9000 }) },
      subRuns: runsOf(sub("old", "api", "Old", { startedAt: 100, status: "done" })),
      posts: postsIn(post("coordinator", "api", "hand-over", "Old", { created_at: 100 })),
    });
    expect(container.querySelectorAll(".org-edge[style]")).toHaveLength(0);
  });

  it("a member waiting in the queue is not working", () => {
    put({ runSummaries: { [CONV]: summary({ live: false }) }, subRuns: runsOf(sub("a", "api", "Build it", { waiting: true })) });
    expect(container.querySelectorAll(".org-orb")).toHaveLength(0);
    expect(sentence()).toBe("Resting");
  });

  it("keeps the style's overlay: the items beside the member that has them, the stopped line, waiting for you", () => {
    put({
      runSummaries: {
        [CONV]: summary({
          stopped: { check: "tests" },
          plan: { revisions: 0, items: [{ text: "Schema", status: "done", by: "api" }, { text: "Export", status: "doing", by: "api" }, { text: "Docs", status: "todo" }] },
        }),
      },
      pendingQuestion: { convId: CONV } as never,
    });
    const dots = [...container.querySelectorAll(".org-node.item")];
    expect(dots.map((d) => d.getAttribute("class"))).toEqual(["org-node item done", "org-node item doing"]);
    expect(dots[1].querySelector("title")?.textContent).toBe("Item 2, in hand: Export");
    expect(container.querySelector("svg.team-svg")!.getAttribute("class")).toContain("stopped");
    expect(container.querySelector(".org-node.you")!.getAttribute("class")).toContain("waiting");
    expect(sentence()).toBe("Now: Poiesis is working; the line is stopped; waiting for you.");
  });

  it("opens a member's task chat when its place is pressed, as the Fleet card does", () => {
    put({ subRuns: runsOf(sub("a", "api", "Build it")) });
    act(() => nodeOf("api")!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(openItem).toHaveBeenCalledWith({ kind: "run", id: "a" });
    // A member with no run has no chat to open: its place is not a button.
    expect(nodeOf("docs")!.getAttribute("role")).toBeNull();
  });
});

describe("Tasks", () => {
  const rows = () => [...container.querySelectorAll(".team-task")];
  const text = (r: Element) => r.querySelector(".team-task-text")?.textContent;
  const holder = (r: Element) => r.querySelector(".team-task-role")?.textContent;
  const word = (r: Element) => r.querySelector(".team-task-word")?.textContent;
  const from = (r: Element) => r.querySelector(".team-task-from")?.textContent;
  const at = (r: Element) => r.querySelector(".team-task-time");
  const last = (r: Element) => r.querySelector(".team-task-last")?.textContent;
  const handOver = post("coordinator", "api", "hand-over", "# Build the charge endpoint\nwith the details", { item: 1 });
  const note = post("api", "coordinator", "note", "The charge   table is locked.", { run_id: "a" });
  const review = post("api", "verifier", "review", "Built it", { item: 1 });
  const docs = post("coordinator", "docs", "hand-over", "Update the README");
  const question = post("verifier", "coordinator", "ask", "Which branch?");
  beforeEach(() => {
    put({
      runSummaries: {
        [CONV]: summary({
          live: false,
          plan: { revisions: 0, items: [{ text: "Charge endpoint", status: "doing", by: "api", review: { state: "checking" } }, { text: "Export", status: "todo", by: "verifier" }] },
        }),
      },
      subRuns: runsOf(sub("a", "api", "Build it", { item: 1, status: "done" }), sub("d", "docs", "Update the README", { startedAt: 6000 })),
      posts: postsIn(handOver, note, review, docs, question),
    });
    act(() => tab("Tasks").click());
  });

  it("lists who was given what: the work, the one who has it, who gave it, and where it is", () => {
    expect(tab("Tasks").getAttribute("aria-selected")).toBe("true");
    expect(container.querySelector("svg.team-svg")).toBeNull();
    // The two hand-overs in the order they were made, then the plan item nobody posted about.
    expect(rows().map(text)).toEqual(["Charge endpoint", "Update the README", "Export"]);
    expect(rows().map(holder)).toEqual(["api", "docs", "verifier"]);
    expect(rows()[0].querySelector(".team-task-n")?.textContent).toBe("1");
    // Where it is: the plan item's word when there is one, else the run's.
    expect(rows().map(word)).toEqual(["being checked", "in hand", "waiting"]);
    expect(rows().map(from)).toEqual(["from coordinator", "from coordinator", "from coordinator"]);
    expect(rows()[0].getAttribute("class")).toContain("checking");
    // When it was handed over; a plan item nobody posted about has no time.
    expect(at(rows()[0])).not.toBeNull();
    expect(at(rows()[2])).toBeNull();
  });

  it("shows the newest word about each piece, and puts questions and notes that are no piece of work under the list", () => {
    expect(last(rows()[0])).toBe("api → verifier, review: Built it");
    expect(last(rows()[1])).toBeUndefined();
    const talk = [...container.querySelectorAll(".team-talk-row")];
    expect(talk.map((r) => r.textContent)).toEqual(["verifier → coordinatorquestionWhich branch?"]);
  });

  it("uses the first line of a hand-over when the work is no plan item", () => {
    put({ runSummaries: { [CONV]: summary({ live: false }) }, posts: postsIn(post("coordinator", "docs", "hand-over", "## Write the guide\nlong instructions")) });
    expect(rows().map(text)).toEqual(["Write the guide"]);
  });

  it("is a piece of work again with its new holder when it is handed over a second time", () => {
    put({ posts: postsIn(handOver, review, post("coordinator", "verifier", "hand-over", "Take item 1 over", { item: 1 })) });
    expect(rows()[0].querySelector(".team-task-role")?.textContent).toBe("verifier");
    expect(rows().filter((r) => text(r) === "Charge endpoint")).toHaveLength(1);
  });

  it("opens the chat of the run that works a piece; one nobody started is no button", () => {
    act(() => rows()[0].querySelector<HTMLButtonElement>("button")!.click());
    expect(openItem).toHaveBeenCalledWith({ kind: "run", id: "a" });
    expect(rows()[2].querySelector("button")).toBeNull();
  });

  it("shows everything about a piece of work on hover: where it is, who, the whole hand-over, all said since", () => {
    vi.useFakeTimers();
    try {
      const row = rows()[0].querySelector<HTMLElement>(".team-task-row")!;
      act(() => row.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })));
      expect(document.querySelector(".team-tip")).toBeNull();
      act(() => {
        vi.advanceTimersByTime(400);
      });
      const tip = document.querySelector<HTMLElement>(".team-tip")!;
      expect(tip.getAttribute("role")).toBe("tooltip");
      expect(row.getAttribute("aria-describedby")).toBe(tip.id);
      expect(tip.querySelector(".team-tip-title")?.textContent).toBe("Charge endpoint");
      expect(tip.querySelector(".team-tip-state")?.textContent).toBe("Item 1 · being checked");
      expect(tip.querySelector(".team-tip-persona")?.textContent).toBe("Atlas");
      expect(tip.querySelector(".team-tip-brief")?.textContent).toBe("# Build the charge endpoint\nwith the details");
      expect([...tip.querySelectorAll(".team-tip-post-who")].map((w) => w.textContent)).toEqual(["api → coordinator", "api → verifier"]);
      expect(tip.querySelector(".team-tip-foot")?.textContent).toBe("Click to open api's chat.");
      act(() => row.dispatchEvent(new MouseEvent("mouseout", { bubbles: true })));
      expect(document.querySelector(".team-tip")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows the card at once when a row is reached with the keyboard, even one that opens nothing", () => {
    vi.useFakeTimers();
    try {
      const row = rows()[2].querySelector<HTMLElement>(".team-task-row")!;
      expect(row.tabIndex).toBe(0);
      act(() => row.focus());
      act(() => {
        vi.advanceTimersByTime(0);
      });
      const tip = document.querySelector(".team-tip")!;
      expect(tip.querySelector(".team-tip-title")?.textContent).toBe("Export");
      expect(tip.querySelector(".team-tip-brief")).toBeNull();
      expect(tip.querySelector(".team-tip-foot")).toBeNull();
      act(() => row.blur());
      expect(document.querySelector(".team-tip")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("says so when nothing was handed over", () => {
    put({ runSummaries: { [CONV]: summary({ live: false }) }, posts: {} });
    expect(container.querySelector(".team-tasks-empty")?.textContent).toBe("Nothing has been handed over yet.");
  });

  it("slides in a piece of work handed over while it is open, and only that one", () => {
    expect(container.querySelectorAll(".team-task.fresh")).toHaveLength(0);
    put({ posts: postsIn(handOver, note, review, docs, question, post("coordinator", "api", "hand-over", "Add a refund")) });
    expect(rows().map((r) => r.classList.contains("fresh"))).toEqual([false, false, true, false]);
  });
});

describe("the replay", () => {
  const handOver = post("coordinator", "api", "hand-over", "Build it", { item: 1 });
  const result = post("api", "coordinator", "result", "Built it", { item: 1 });
  const review = post("api", "verifier", "review", "Built it", { item: 1 });
  const width = (key: string) => parseFloat(/stroke-width:\s*([\d.]+)/.exec(lineOf(key).getAttribute("style") ?? "")?.[1] ?? "0");
  const range = () => container.querySelector<HTMLInputElement>(".team-replay-range")!;
  const slide = (n: number) => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    act(() => {
      set.call(range(), String(n));
      range().dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  const button = (label: string) => container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;

  beforeEach(() => {
    put({ runSummaries: { [CONV]: summary() }, subRuns: runsOf(sub("a", "api", "Build it", { item: 1, status: "done" })), posts: postsIn(handOver, result, review) });
  });

  it("has no scrubber before there is a post, and is Live when there is", () => {
    put({ posts: {} });
    expect(container.querySelector(".team-replay")).toBeNull();
    put({ posts: postsIn(handOver, result, review) });
    expect(range().max).toBe("3");
    expect(range().value).toBe("3");
    expect(container.querySelector(".team-replay-live")!.getAttribute("aria-pressed")).toBe("true");
  });

  it("plays the posts in order: the lines are as wide as the posts so far, and the sentence says the latest", () => {
    slide(0);
    expect(sentence()).toBe("Replay: before the first post, 3 to come.");
    expect(width("api|coordinator")).toBe(0);
    slide(1);
    expect(sentence()).toBe("Replay, 1 of 3: coordinator to api, hand-over, item 1: “Build it”");
    expect(width("api|coordinator")).toBeGreaterThan(0);
    expect(width("api|verifier")).toBe(0);
    expect(container.querySelector(".org-line.current")!.getAttribute("data-line")).toBe("api|coordinator");
    slide(3);
    // The last position is Live again: the run's own sentence, and nothing is "current".
    expect(container.querySelector(".org-line.current")).toBeNull();
    expect(sentence()).toBe("Now: Poiesis is working.");
    expect(width("api|verifier")).toBeGreaterThan(0);
  });

  it("has one tick per post on the rail, filled once passed, in the sender's hue", () => {
    const ticks = () => [...container.querySelectorAll(".team-replay-tick")];
    expect(ticks()).toHaveLength(3);
    expect(ticks().every((t) => t.classList.contains("past"))).toBe(true);
    // The coordinator is Poiesis (ink); api is Atlas, a persona with a hue.
    expect(ticks()[0].getAttribute("data-hue")).toBeNull();
    expect(ticks()[1].getAttribute("data-hue")).toMatch(/^[1-8]$/);
    slide(1);
    expect(ticks().map((t) => t.getAttribute("class"))).toEqual(["team-replay-tick past at", "team-replay-tick", "team-replay-tick"]);
  });

  it("draws the line the replay is on in the hue of the one who sent the post", () => {
    slide(1);
    // The hand-over came from Poiesis: ink, no hue.
    expect(container.querySelector(".org-line.current")!.getAttribute("data-hue")).toBeNull();
    slide(2);
    expect(container.querySelector(".org-line.current")!.getAttribute("data-line")).toBe("api|coordinator");
    expect(container.querySelector(".org-line.current")!.getAttribute("data-hue")).toMatch(/^[1-8]$/);
    expect(container.querySelector(".org-sentence")!.getAttribute("class")).toContain("replay");
  });

  it("shows nobody working while it replays, and Live brings the orbs back", () => {
    put({ subRuns: runsOf(sub("a", "api", "Build it", { item: 1 })) });
    expect(container.querySelectorAll(".org-orb")).toHaveLength(1);
    slide(1);
    expect(container.querySelectorAll(".org-orb")).toHaveLength(0);
    act(() => container.querySelector<HTMLButtonElement>(".team-replay-live")!.click());
    expect(container.querySelectorAll(".org-orb")).toHaveLength(1);
    expect(range().value).toBe("3");
  });

  it("goes back to the first post, and plays on from there one post at a time", () => {
    vi.useFakeTimers();
    try {
      act(() => button("Back to the first post").click());
      expect(sentence()).toBe("Replay: before the first post, 3 to come.");
      act(() => button("Play").click());
      act(() => {
        vi.advanceTimersByTime(900);
      });
      expect(sentence()).toContain("Replay, 1 of 3");
      for (let tick = 0; tick < 3; tick++) {
        act(() => {
          vi.advanceTimersByTime(900);
        });
      }
      // It ended on Live.
      expect(sentence()).toBe("Now: Poiesis is working.");
      expect(container.querySelector(".team-replay-live")!.getAttribute("aria-pressed")).toBe("true");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("the Members tab", () => {
  const members = () => [...container.querySelectorAll(".team-member")];

  it("groups by member: face, role, persona, what it is doing, its chat, its items; a plain helper has (·)", () => {
    put({
      runSummaries: {
        [CONV]: summary({ plan: { revisions: 0, items: [{ text: "Export", status: "doing", by: "api" }] } }),
      },
      subRuns: runsOf(sub("a", "api", "Build it", { item: 1 }), sub("d", "docs", "Write", { status: "done", text: "ok" })),
    });
    act(() => tab("Members").click());
    expect(tab("Members").getAttribute("aria-selected")).toBe("true");
    expect(container.querySelector("svg.team-svg")).toBeNull();
    expect(members().map((m) => m.querySelector(".team-member-role")?.textContent)).toEqual(["api", "docs", "verifier"]);
    expect(members().map((m) => m.querySelector(".team-member-persona")?.textContent)).toEqual(["Atlas", "helper", "Vera"]);
    // The face: a letter for a persona, (·) for the helper.
    expect(members()[0].querySelector(".persona-glyph")).not.toBeNull();
    expect(members()[1].querySelector(".member-glyph-helper")?.textContent).toBe("·");
    expect(members()[0].querySelector(".team-member-doing")?.textContent).toBe("getting started");
    expect(members()[1].querySelector(".team-member-doing")?.textContent).toMatch(/^done/);
    expect(members()[2].querySelector(".team-member-doing")?.textContent).toBe("has not started");
    expect(members()[0].querySelector(".team-member-items")?.textContent).toBe("Item 1, in hand: Export");
    // Its task chat opens; a member with none has no button.
    act(() => members()[0].querySelector<HTMLButtonElement>("button")!.click());
    expect(openItem).toHaveBeenCalledWith({ kind: "run", id: "a" });
    expect(members()[2].querySelector("button")).toBeNull();
  });

  it("lists helpers that are not members of the team under their own heading", () => {
    put({ subRuns: runsOf(sub("h", undefined, "Look it up", { agent: "general" })) });
    act(() => tab("Members").click());
    expect(members()[members().length - 1].textContent).toContain("helpers");
    expect(members()[members().length - 1].textContent).toContain("Look it up");
  });
});

describe("reduced motion", () => {
  it("says in words every state the orb and the lines show", () => {
    put({ runSummaries: { [CONV]: summary() }, subRuns: runsOf(sub("a", "api", "Build it", { item: 2 })) });
    const before = sentence();
    put({ subRuns: runsOf(sub("a", "api", "Build it", { item: 2, status: "done" })) });
    // The drawing lost its orb; the sentence moved on from the member to the coordinator.
    expect(container.querySelectorAll(".org-orb")).toHaveLength(0);
    expect(sentence()).not.toBe(before);
    expect(sentence()).toBe("Now: Poiesis is working.");
  });
});

describe("a session no team leads", () => {
  it("is drawn as it always was", async () => {
    useAppStore.setState({ conversations: [{ id: CONV, title: "t", updatedAt: 0, messages: [], leader: "poiesis", personaId: null }] } as never);
    await act(async () => {
      root.render(<AgentsPanel conversationId={CONV} />);
    });
    expect(container.querySelector(".team-coord")).toBeNull();
    expect(container.querySelector(".org-head")?.textContent).toContain("Ad hoc");
    expect(container.querySelector("[role=tab]")).toBeNull();
  });
});
