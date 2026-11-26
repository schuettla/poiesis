/**
 * `TEAM-14`: a team member's run on the store. `sub_spawned` says which place the run is, it waits
 * in the team's queue until `member_started`, and it is not working once it ended. Driven through
 * the real stream handler by scripting a resumed run's events.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, TeamPost } from "./api";

type Script = (emit: (e: AgentEvent) => void) => void | Promise<void>;
let script: Script = () => {};

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  resumeRun: async (_conv: string, onEvent: (e: AgentEvent) => void) => {
    await script(onEvent);
    return true;
  },
}));

import { useAppStore } from "./store";

const started: AgentEvent = { type: "run_started", run_id: "r1", max_steps: 12, context_window: null, tier: "full" };
const spawned = (id: string, member?: string, item?: number): AgentEvent => ({
  type: "sub_spawned",
  run_id: id,
  conversation_id: `child-${id}`,
  agent: member ?? "general",
  task: "do it",
  index: 0,
  ...(member ? { member } : {}),
  ...(item ? { item } : {}),
});

beforeEach(() => {
  const model = useAppStore.getState().models[0];
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], leader: "team:shop", personaId: null }],
    activeConversationId: "c1",
    selectedModelId: model.id,
    engineReady: true,
    loadedModelId: model.id,
    toolsEnabled: false,
    turnModifiers: {},
    runSummaries: {},
    subRuns: {},
    posts: {},
    busy: false,
    presence: "idle",
  } as never);
});

const runs = () => useAppStore.getState().subRuns;

describe("a member's run (TEAM-14)", () => {
  it("knows its place and waits in the queue until the member started", async () => {
    script = (emit) => {
      emit(started);
      emit(spawned("a", "api", 2));
      emit(spawned("b", "api", 3));
      emit(spawned("h"));
    };
    await useAppStore.getState().resumeLastRun();
    expect(runs().a).toMatchObject({ member: "api", item: 2, waiting: true, status: "running" });
    expect(runs().h.member).toBeUndefined();
    expect(runs().h.waiting).toBeUndefined();

    script = (emit) => {
      emit(started);
      emit({ type: "member_started", run_id: "a", member: "api", item: 2 });
    };
    await useAppStore.getState().resumeLastRun();
    // The run that left the queue is the one named; the second still waits.
    expect(runs().a.waiting).toBe(false);
    expect(runs().b.waiting).toBe(true);
  });

  it("is no longer waiting once it ended, however it ended", async () => {
    script = (emit) => {
      emit(started);
      emit(spawned("a", "api"));
      emit({ type: "sub_ended", run_id: "a", status: "stopped", stop_reason: "budget", summary: "", steps: 0, ms: 5 });
    };
    await useAppStore.getState().resumeLastRun();
    expect(runs().a).toMatchObject({ status: "stopped", stopReason: "budget", waiting: false });
  });
});

describe("the board (TEAM-15)", () => {
  const post = (id: string, at: number, over: Partial<TeamPost> = {}): TeamPost => ({
    id,
    conversation_id: "c1",
    run_id: null,
    from_member: "api",
    to_member: "coordinator",
    kind: "note",
    item: null,
    reply_to: null,
    text: id,
    created_at: at,
    delivered_at: null,
    ...over,
  });
  const board = () => useAppStore.getState().posts.c1?.map((p) => p.id);

  it("takes a post from the turn's stream and from a member's own events, once each, oldest first", async () => {
    script = (emit) => {
      emit(started);
      emit(spawned("a", "api"));
      emit({ type: "posted", post: post("p2", 20) });
      emit({ type: "sub", run_id: "a", event: { type: "posted", post: post("p1", 10) } });
      // The same post again, from a reload: not twice.
      emit({ type: "posted", post: post("p2", 20) });
    };
    await useAppStore.getState().resumeLastRun();
    expect(board()).toEqual(["p1", "p2"]);
    // A member's post is the board's, not a line on the member's card.
    expect(runs().a.text).toBe("");
  });
});

describe("a helper waiting for a place to change, and the proof of a job (PRF-5, PRF-2)", () => {
  it("shows why it waits, and is working again when it has its place", async () => {
    script = (emit) => {
      emit(started);
      emit(spawned("a"));
      emit({ type: "sub_waiting", run_id: "a", because: "Waiting: Codex is changing src/a.rs." });
    };
    await useAppStore.getState().resumeLastRun();
    expect(runs().a).toMatchObject({ status: "queued", queuedBecause: "Waiting: Codex is changing src/a.rs." });

    script = (emit) => {
      emit(started);
      emit(spawned("b"));
      emit({ type: "sub_waiting", run_id: "b", because: "Waiting: Codex is changing this folder." });
      emit({ type: "sub_waiting", run_id: "b" });
    };
    await useAppStore.getState().resumeLastRun();
    expect(runs().b.status).toBe("running");
    expect(runs().b.queuedBecause).toBeUndefined();
  });

  it("an agent app that has its place still waits for a free slot, so it is not called working here", async () => {
    script = (emit) => {
      emit(started);
      emit({
        type: "sub_spawned",
        run_id: "e",
        conversation_id: "child-e",
        agent: "Codex",
        task: "do it",
        index: 0,
        engine: { id: "codex", name: "Codex", paid_by: "plan", because: "U1", because_text: null, can_steer: false },
      });
      emit({ type: "sub_waiting", run_id: "e", because: "Waiting: Claude Code is changing this folder." });
      emit({ type: "sub_waiting", run_id: "e" });
    };
    await useAppStore.getState().resumeLastRun();
    expect(runs().e.status).toBe("queued");
    expect(runs().e.queuedBecause).toBeUndefined();
  });

  it("keeps what the job changed when it ends", async () => {
    script = (emit) => {
      emit(started);
      emit(spawned("a"));
      emit({
        type: "sub_ended",
        run_id: "a",
        status: "done",
        stop_reason: "completed",
        summary: "ok",
        steps: 3,
        ms: 5,
        evidence: { changed: 2, check: "cargo test", state: "failed", left: ["a stub in x.rs"] },
      });
    };
    await useAppStore.getState().resumeLastRun();
    expect(runs().a.evidence).toEqual({ changed: 2, check: "cargo test", state: "failed", left: ["a stub in x.rs"] });
    expect(runs().a.queuedBecause).toBeUndefined();
  });
});
