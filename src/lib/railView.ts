/**
 * How the Rail shows its chats beyond the date groups: what to group them by, and the filters
 * the funnel menu offers besides "Led by" (which lives in the store as `leaderFilter`). Pure,
 * so the Rail only has to ask.
 */
import type { StopReason } from "./api";
import type { ChatState } from "./chatState";
import { leaderOf } from "./leaders";
import type { Conversation, Project } from "./types";

export type RailGroup = "date" | "status" | "leader" | "folder";

export const RAIL_GROUPS: { id: RailGroup; label: string }[] = [
  { id: "date", label: "Date" },
  { id: "status", label: "Status" },
  { id: "leader", label: "Led by" },
  { id: "folder", label: "Folder" },
];

export type OutputKind = "artifacts" | "files" | "helpers";

/** `PAR-11`: one status per chat, as the funnel and the Status groups read it: what it does
 * now, else how its last answer ended, else whether it changed while you were away. */
export type RailStatus = "needs-you" | "running" | "held" | "stopped" | "failed" | "done-unseen" | "idle";

/** First rule that is true wins: a live state, then the outcome, then unread. A stopped chat
 * you continue is Running again, and a stopped chat you have read is still Stopped early. */
export function railStatus(live: ChatState, outcome: StopReason | null, unread: boolean): RailStatus {
  if (live === "needs-you" || live === "held" || live === "running") return live;
  if (outcome === "error" || live === "failed") return "failed";
  // A reply cut off by the voice (`interrupted`) was stopped by you speaking, not by a limit.
  if (outcome && outcome !== "completed" && outcome !== "interrupted") return "stopped";
  return unread || live === "done-unseen" ? "done-unseen" : "idle";
}

export interface RailFilters {
  /** Any of these; empty means the filter is off. */
  status: RailStatus[];
  output: OutputKind | null;
  /** A folder's path. */
  folder: string | null;
  /** A project's id, or `NO_PROJECT` for the loose chats. */
  project: string | null;
}
export const NO_RAIL_FILTERS: RailFilters = { status: [], output: null, folder: null, project: null };
export const NO_PROJECT = "none";

/** The states worth filtering for, in the order a person cares about them. */
export const STATUS_ORDER: Exclude<RailStatus, "idle">[] = [
  "running",
  "needs-you",
  "held",
  "stopped",
  "failed",
  "done-unseen",
];
export const STATUS_WORD: Record<RailStatus, string> = {
  running: "Running",
  "needs-you": "Waiting for you",
  held: "Waiting on something else",
  stopped: "Stopped early",
  failed: "Failed",
  "done-unseen": "Changed while away",
  idle: "Idle",
};
/** The three states of a chat that is not at rest, which one click picks together. */
export const LIVE_STATUSES: RailStatus[] = ["running", "needs-you", "held"];
export const LIVE_WORDS = "Working or waiting now";
export const isLiveSet = (status: RailStatus[]): boolean =>
  status.length === LIVE_STATUSES.length && LIVE_STATUSES.every((s) => status.includes(s));

/** What the chip over the list says for the picked states, or null when none is. */
export function statusChipWords(status: RailStatus[]): string | null {
  if (!status.length) return null;
  if (isLiveSet(status)) return LIVE_WORDS;
  if (status.length > 2) return `${status.length} states`;
  const order: RailStatus[] = [...STATUS_ORDER, "idle"];
  return order.filter((s) => status.includes(s)).map((s) => STATUS_WORD[s]).join(" or ");
}

const NONE_WORDS: Record<RailStatus, string> = {
  running: "No chats are running.",
  "needs-you": "No chats wait for you.",
  held: "No chats wait on anything.",
  stopped: "No chats stopped early.",
  failed: "No chats failed.",
  "done-unseen": "No unread chats.",
  idle: "No idle chats.",
};
/** What an empty list says when one state is all that is filtered for. */
export const noChatsWords = (status: RailStatus[]): string =>
  status.length === 1 ? NONE_WORDS[status[0]] : "No chats match.";
export const OUTPUT_ORDER: OutputKind[] = ["artifacts", "files", "helpers"];
export const OUTPUT_WORD: Record<OutputKind, string> = {
  artifacts: "Made artifacts",
  files: "Changed files",
  helpers: "Used helpers",
};

export function hasRailFilters(f: RailFilters): boolean {
  return f.status.length > 0 || f.output !== null || f.folder !== null || f.project !== null;
}

/** The folder a chat works in: its own, or its project's. */
export function folderOf(c: Conversation, projects: Project[]): string | null {
  return c.folderPath ?? (c.projectId ? projects.find((p) => p.id === c.projectId)?.rootPath : null) ?? null;
}
export function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** What the filters and groups need to know about the world, gathered once by the Rail. */
export interface RailContext {
  projects: Project[];
  status: (id: string) => RailStatus;
  /** The chat on screen, once the status filter has shown it: it stays in the list until
   * you open another, even when its state no longer matches. */
  keep?: string | null;
  /** `[artifacts made, files changed]` by chat. Empty until read. */
  outputs: Record<string, [number, number]>;
  /** Chats that have helpers under them. */
  withHelpers: (id: string) => boolean;
}

export function hasOutput(c: Conversation, kind: OutputKind, ctx: RailContext): boolean {
  if (kind === "helpers") return ctx.withHelpers(c.id);
  const [artifacts, files] = ctx.outputs[c.id] ?? [0, 0];
  return kind === "artifacts" ? artifacts > 0 : files > 0;
}

/** Every filter that is on must hold. */
export function matchesRailFilters(c: Conversation, f: RailFilters, ctx: RailContext): boolean {
  if (f.status.length && c.id !== ctx.keep && !f.status.includes(ctx.status(c.id))) return false;
  if (f.output && !hasOutput(c, f.output, ctx)) return false;
  if (f.folder && folderOf(c, ctx.projects) !== f.folder) return false;
  if (f.project && (f.project === NO_PROJECT ? !!c.projectId : c.projectId !== f.project)) return false;
  return true;
}

export interface RailGroupOf<T> {
  label: string;
  items: T[];
}

/** The chats in the groups of one mode, each group in the order the chats came in. Empty groups
 * are left out. (`date` is `groupByBucket`'s, which also knows the calendar.) */
export function groupChats(
  chats: Conversation[],
  mode: Exclude<RailGroup, "date">,
  ctx: Pick<RailContext, "projects" | "status"> & { leaders: { id: string; name: string }[] }
): RailGroupOf<Conversation>[] {
  const buckets = new Map<string, Conversation[]>();
  const add = (key: string, c: Conversation) => {
    const list = buckets.get(key);
    if (list) list.push(c);
    else buckets.set(key, [c]);
  };

  if (mode === "status") {
    for (const c of chats) add(ctx.status(c.id), c);
    return [...STATUS_ORDER, "idle" as const]
      .filter((s) => buckets.has(s))
      .map((s) => ({ label: STATUS_WORD[s], items: buckets.get(s)! }));
  }

  if (mode === "leader") {
    const known = new Set(ctx.leaders.map((l) => l.id));
    // A leader that is gone is read as Poiesis, as the row's marker does.
    for (const c of chats) add(known.has(leaderOf(c)) ? leaderOf(c) : "poiesis", c);
    return ctx.leaders.filter((l) => buckets.has(l.id)).map((l) => ({ label: l.name, items: buckets.get(l.id)! }));
  }

  for (const c of chats) add(folderOf(c, ctx.projects) ?? "", c);
  const named = [...buckets.keys()]
    .filter((k) => k !== "")
    .sort((a, b) => folderName(a).localeCompare(folderName(b)));
  return [...named, ...(buckets.has("") ? [""] : [])].map((k) => ({
    label: k === "" ? "No folder" : folderName(k),
    items: buckets.get(k)!,
  }));
}
