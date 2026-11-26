/**
 * How the Rail shows its chats beyond the date groups: what to group them by, and the filters
 * the funnel menu offers besides "Led by" (which lives in the store as `leaderFilter`). Pure,
 * so the Rail only has to ask.
 */
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

export interface RailFilters {
  status: ChatState | null;
  output: OutputKind | null;
  /** A folder's path. */
  folder: string | null;
  /** A project's id, or `NO_PROJECT` for the loose chats. */
  project: string | null;
}
export const NO_RAIL_FILTERS: RailFilters = { status: null, output: null, folder: null, project: null };
export const NO_PROJECT = "none";

/** The states worth filtering for, in the order a person cares about them. */
export const STATUS_ORDER: Exclude<ChatState, "idle">[] = ["needs-you", "running", "failed", "done-unseen"];
export const STATUS_WORD: Record<ChatState, string> = {
  "needs-you": "Waiting for you",
  running: "Running",
  failed: "Failed",
  "done-unseen": "Changed while away",
  idle: "Everything else",
};
export const OUTPUT_ORDER: OutputKind[] = ["artifacts", "files", "helpers"];
export const OUTPUT_WORD: Record<OutputKind, string> = {
  artifacts: "Made artifacts",
  files: "Changed files",
  helpers: "Used helpers",
};

export function hasRailFilters(f: RailFilters): boolean {
  return f.status !== null || f.output !== null || f.folder !== null || f.project !== null;
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
  status: (id: string) => ChatState;
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
  if (f.status && ctx.status(c.id) !== f.status) return false;
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
