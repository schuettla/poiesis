import { stillWorking, type Persona, type PlanView, type Team, type TeamPost } from "./api";
import { itemDot } from "./orgState";
import { lookOf } from "./personaLook";
import type { RunSummary, SubRun } from "./types";

/**
 * `TEAM-14`: what a team is doing right now, read from the run's own events and the run rows
 * and nothing else (`TEAMS_PLAN` II.6). The chart, the Members tab and the sentence under the
 * chart all draw from this, so none of them can claim something that did not happen.
 *
 * `TEAM-15`: the line widths and the Board are read from the posts the loop and the `post` tool wrote
 * (`posts` table). Everything else (who is working, what it is doing) is still read from the run rows. */

/** A run of a team member, read from its task: the loop writes "Review item 3 by api: ..." for a
 * review and "Sent back by verifier on item 3: ..." for a send-back (`team.rs`). */
export interface MemberRun {
  kind: "hand-over" | "review" | "send-back";
  /** The plan item, from 1, when the run has one. */
  item?: number;
  /** A review: whose work it looks at. A send-back: who sent it back. */
  other?: string;
}

const REVIEW = /^Review item (\d+) by ([a-z0-9_-]+):/;
const BACK = /^Sent back by ([a-z0-9_-]+) on item (\d+):/;

export function readRun(run: SubRun): MemberRun {
  const review = REVIEW.exec(run.task);
  if (review) return { kind: "review", item: Number(review[1]), other: review[2] };
  const back = BACK.exec(run.task);
  if (back) return { kind: "send-back", item: Number(back[2]), other: back[1] };
  return { kind: "hand-over", ...(run.item ? { item: run.item } : {}) };
}

/** The key of the drawn line between two places, the same one `pairsOf` gives it. */
export const pairKey = (a: string, b: string): string => [a, b].sort().join("|");

/**
 * The runs that belong to the task on screen: from the run the window knows (`summary`), else,
 * after the app was opened again, the runs of the newest turn that handed work out.
 */
export function taskRuns(
  runs: SubRun[],
  summary: Pick<RunSummary, "startedAt"> | undefined,
  messages: { subRunIds?: string[] }[]
): SubRun[] {
  if (summary) return runs.filter((r) => r.startedAt >= summary.startedAt - 1000);
  const last = [...messages].reverse().find((m) => m.subRunIds?.length)?.subRunIds ?? [];
  return runs.filter((r) => last.includes(r.runId));
}

/** Roles that are working this moment: out of the queue and not finished. */
export const workingRoles = (runs: SubRun[]): string[] => [
  ...new Set(runs.filter((r) => r.member && stillWorking(r.status) && !r.waiting).map((r) => r.member!)),
];

/**
 * The posts of the task on screen: the ones written from the moment the window's run began (`summary`),
 * else, after the app was opened again, from the moment the earliest run of that task began. A little
 * before it, because a question the coordinator posts is written before the member it wakes starts.
 */
export function taskPosts(posts: TeamPost[], summary: { startedAt: number } | undefined, runs: SubRun[]): TeamPost[] {
  const from = summary ? summary.startedAt : runs.length ? Math.min(...runs.map((r) => r.startedAt)) : Infinity;
  return posts.filter((p) => p.created_at >= from - 1000);
}

/** How many posts went along each line (by `pairKey`) in these posts. */
export function postsOf(posts: TeamPost[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const p of posts) {
    const k = pairKey(p.from_member, p.to_member);
    out[k] = (out[k] ?? 0) + 1;
  }
  return out;
}

const KIND_WORD: Record<TeamPost["kind"], string> = {
  "hand-over": "hand-over",
  result: "result",
  review: "review",
  "sent-back": "sent back",
  ask: "question",
  answer: "answer",
  report: "report",
  note: "note",
};

/** A post's kind in words: "hand-over", "sent back", "question". */
export const kindWord = (kind: TeamPost["kind"]): string => KIND_WORD[kind] ?? kind;

/** One post as a line, the way the Board and the replay say it: `api to verifier, review, item 3: "…"`. */
export function postLine(p: TeamPost, max = 90): string {
  const text = p.text.replace(/\s+/g, " ").trim();
  const said = text.length > max ? `${text.slice(0, max - 1)}…` : text;
  const item = p.item ? `, item ${p.item}` : "";
  return `${p.from_member} to ${p.to_member}, ${KIND_WORD[p.kind] ?? p.kind}${item}${said ? `: “${said}”` : ""}`;
}

/** The roles a post is between, for the Board's filter. */
export const postMembers = (posts: TeamPost[]): string[] => [...new Set(posts.flatMap((p) => [p.from_member, p.to_member]))];

/** The plan's items, by the role that has them: the dots drawn next to a member. */
export function itemsOf(plan: PlanView | undefined, roles: string[]): Record<string, { n: number; text: string; state: string; word: string }[]> {
  const out: Record<string, { n: number; text: string; state: string; word: string }[]> = {};
  plan?.items.forEach((item, i) => {
    if (!item.by || !roles.includes(item.by)) return;
    const state =
      item.review?.state === "checking" ? "checking" : item.review?.state === "sent_back" && item.status === "doing" ? "sent-back" : item.status;
    (out[item.by] ??= []).push({ n: i + 1, text: item.text, state, word: itemDot(item).word });
  });
  return out;
}

/** What a working run is doing, as a clause: "verifier is looking at item 3 from api". */
export function doingWords(run: SubRun): string {
  const role = run.member ?? run.agent;
  const read = readRun(run);
  if (read.kind === "review") return `${role} is looking at ${read.item ? `item ${read.item}` : "the work"} from ${read.other}`;
  if (read.kind === "send-back") return `${role} is fixing ${read.item ? `item ${read.item}` : "its work"} after ${read.other} sent it back`;
  return read.item ? `${role} is working on item ${read.item}` : `${role} is working`;
}

/** The "Now:" sentence under the chart, which is the only thing that says it all under
 * reduced motion. Empty when nothing is going on. */
export function nowSentence(f: {
  runs: SubRun[];
  live: boolean;
  asking: boolean;
  stopped: boolean;
  /** What the coordinator is called in a sentence. */
  coordinator: string;
}): string {
  const working = f.runs
    .filter((r) => r.member && stillWorking(r.status) && !r.waiting)
    .sort((a, b) => a.startedAt - b.startedAt);
  const parts = working.slice(0, 3).map(doingWords);
  if (working.length > 3) parts.push("others are working");
  if (parts.length === 0 && f.live) parts.push(`${f.coordinator} is working`);
  if (f.stopped) parts.push("the line is stopped");
  if (f.asking) parts.push("waiting for you");
  return parts.length ? `Now: ${parts.join("; ")}.` : "";
}

/** Who a place is: a persona, Poiesis, or a plain helper. */
export type Who = { kind: "persona"; name: string; persona?: Pick<Persona, "id" | "name" | "look"> } | { kind: "poiesis"; name: string } | { kind: "helper"; name: null };

export function whoIs(team: Pick<Team, "coordinator" | "members">, role: string, personas: Pick<Persona, "id" | "name" | "look">[]): Who {
  const ref = role === "coordinator" ? team.coordinator : team.members.find((m) => m.role === role)?.persona;
  if (!ref || ref === "helper") return { kind: "helper", name: null };
  if (ref === "poiesis") return { kind: "poiesis", name: "Poiesis" };
  const persona = personas.find((p) => `persona:${p.id}` === ref);
  return { kind: "persona", name: persona?.name ?? "a persona that is gone", persona };
}

/** The hue a place is drawn in (1 to 8), when it is a persona: its posts carry it too. You, Poiesis
 * and a plain helper have none and are drawn in ink. */
export function roleHue(team: Pick<Team, "coordinator" | "members">, role: string, personas: Pick<Persona, "id" | "name" | "look">[]): number | undefined {
  if (role === "you" || role === "all") return undefined;
  const who = whoIs(team, role, personas);
  return who.kind === "persona" && who.persona ? lookOf(who.persona).hue : undefined;
}

/** The Fleet card's name for a run: "api · Atlas", or just the role for a plain helper. A run
 * that is not a team member keeps its agent type. */
export function runLabel(run: SubRun, team: Pick<Team, "coordinator" | "members"> | null | undefined, personas: Pick<Persona, "id" | "name" | "look">[]): string {
  if (!run.member || !team) return run.agent;
  const who = whoIs(team, run.member, personas);
  return who.name ? `${run.member} · ${who.name}` : run.member;
}

/** Where a handed-over piece of work is. The same states as a plan item's dot, so the Tasks
 * list and the chart say the same thing; `stopped` is a run that ended without finishing. */
export type TaskState = "todo" | "doing" | "checking" | "sent-back" | "done" | "dropped" | "stopped";

/** `TEAM-16`: one piece of work handed to someone in this task. */
export interface TeamTask {
  key: string;
  /** The plan item (from 1), when it is one. */
  item: number | null;
  /** What it is: the plan item's words, else the first line of the hand-over. */
  text: string;
  /** Who handed it over, and to whom (the one who has it now, after a send-back too). */
  from: string;
  to: string;
  /** When it was handed over; a plan item nobody posted about has none. */
  at: number | null;
  state: TaskState;
  /** The state in words: "waiting", "in hand", "being checked", "sent back", "done". */
  word: string;
  /** The newest post about it after the hand-over: a result, a review, a send-back, a question. */
  last?: TeamPost;
  /** Everything said about it after the hand-over, oldest first: the details card shows it. */
  thread: TeamPost[];
  /** The whole hand-over, as it was written (the newest one, after a second hand-over). */
  brief?: string;
  /** The run that works it, when there is one: pressing the task opens its chat. */
  runId?: string;
}

const firstLine = (s: string) => s.replace(/^[#>\s*-]+/, "").split("\n")[0].trim();

/**
 * `TEAM-16`: the task as a list of who was given what. Every hand-over is a task (a second
 * hand-over of the same item, after a send-back, is the same task with its new holder), and a plan
 * item someone has without a post about it is one too. Where it is comes from the plan item when
 * there is one, else from the run that took it, else from whether a result came back. The posts
 * that belong to no task (a question, a note) are `loose`.
 */
export function tasksOf(posts: TeamPost[], plan: PlanView | undefined, runs: SubRun[]): { tasks: TeamTask[]; loose: TeamPost[] } {
  const byKey = new Map<string, Omit<TeamTask, "thread"> & { post?: TeamPost }>();
  const used = new Set<string>();
  for (const p of posts) {
    if (p.kind !== "hand-over") continue;
    const key = p.item ? `item-${p.item}` : p.id;
    const had = byKey.get(key);
    used.add(p.id);
    if (had) Object.assign(had, { from: p.from_member, to: p.to_member, post: p });
    else byKey.set(key, { key, item: p.item, text: firstLine(p.text), from: p.from_member, to: p.to_member, at: p.created_at, state: "todo", word: "", post: p });
  }
  plan?.items.forEach((it, i) => {
    const key = `item-${i + 1}`;
    const had = byKey.get(key);
    if (had) had.text = it.text;
    else if (it.by) byKey.set(key, { key, item: i + 1, text: it.text, from: "coordinator", to: it.by, at: null, state: "todo", word: "" });
  });

  const tasks: TeamTask[] = [...byKey.values()].map(({ post, ...t }) => {
    // The run that works it: for an item the newest one (after a send-back it is the fix), else the
    // first one its holder started after the hand-over.
    const since = post?.created_at ?? -Infinity;
    const run = t.item
      ? [...runs].reverse().find((r) => r.member === t.to && r.item === t.item)
      : runs.find((r) => r.member === t.to && r.startedAt >= since - 1000);
    // What came after: a post about the same item, one its run wrote, or a reply to the hand-over.
    const after = posts.filter(
      (p) =>
        p.id !== post?.id &&
        p.kind !== "hand-over" &&
        ((t.item && p.item === t.item) || (run && p.run_id === run.runId) || (post && p.reply_to === post.id))
    );
    after.forEach((p) => used.add(p.id));
    const last = after.length ? after.reduce((a, b) => (b.created_at >= a.created_at ? b : a)) : undefined;
    const planItem = t.item ? plan?.items[t.item - 1] : undefined;
    let state: TaskState;
    let word: string;
    if (planItem) {
      state =
        planItem.review?.state === "checking"
          ? "checking"
          : planItem.review?.state === "sent_back" && planItem.status === "doing"
            ? "sent-back"
            : planItem.status === "doing" || planItem.status === "done" || planItem.status === "dropped"
              ? planItem.status
              : "todo";
      word = itemDot(planItem).word;
    } else if (run) {
      state = run.status === "done" ? "done" : run.status === "error" || run.status === "stopped" ? "stopped" : stillWorking(run.status) && !run.waiting ? "doing" : "todo";
      word = { done: "done", stopped: "stopped", doing: "in hand", todo: "waiting" }[state as "done" | "stopped" | "doing" | "todo"];
    } else if (last?.kind === "result" && last.from_member === t.to) {
      [state, word] = ["done", "done"];
    } else {
      [state, word] = ["todo", "waiting"];
    }
    const thread = [...after].sort((a, b) => a.created_at - b.created_at);
    return { ...t, state, word, thread, ...(post ? { brief: post.text.trim() } : {}), ...(last ? { last } : {}), ...(run ? { runId: run.runId } : {}) };
  });
  tasks.sort((a, b) => (a.at ?? Infinity) - (b.at ?? Infinity) || (a.item ?? 0) - (b.item ?? 0));
  return { tasks, loose: posts.filter((p) => !used.has(p.id)) };
}

/** The width a line is drawn with once posts went along it: the only quantity on the chart. */
export const postWidth = (n: number): number => 1.4 + 0.6 * Math.min(n, 6);
