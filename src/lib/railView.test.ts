import { describe, expect, it } from "vitest";
import type { ChatState } from "./chatState";
import {
  groupChats,
  hasRailFilters,
  matchesRailFilters,
  NO_PROJECT,
  NO_RAIL_FILTERS,
  type RailContext,
} from "./railView";
import type { Conversation, Project } from "./types";

const conv = (id: string, over: Partial<Conversation> = {}): Conversation => ({
  id,
  title: id,
  updatedAt: 0,
  messages: [],
  ...over,
});
const shop = { id: "s", name: "Shop", rootPath: "C:\\work\\shop" } as Project;
const book = { id: "b", name: "Book", rootPath: null } as Project;

const ctx = (states: Record<string, ChatState> = {}, outputs: RailContext["outputs"] = {}, helpers: string[] = []): RailContext => ({
  projects: [shop, book],
  status: (id) => states[id] ?? "idle",
  outputs,
  withHelpers: (id) => helpers.includes(id),
});

describe("filters", () => {
  const chats = [conv("a", { projectId: "s" }), conv("b", { projectId: "b" }), conv("c"), conv("d", { folderPath: "C:\\x\\notes" })];
  const ids = (f: Partial<typeof NO_RAIL_FILTERS>, c = ctx()) =>
    chats.filter((x) => matchesRailFilters(x, { ...NO_RAIL_FILTERS, ...f }, c)).map((x) => x.id);

  it("are all off by default", () => {
    expect(hasRailFilters(NO_RAIL_FILTERS)).toBe(false);
    expect(ids({})).toEqual(["a", "b", "c", "d"]);
  });

  it("match a status", () => {
    expect(ids({ status: "running" }, ctx({ c: "running" }))).toEqual(["c"]);
  });

  it("match what a chat made, from artifacts, changed files or helpers", () => {
    const c = ctx({}, { a: [2, 0], d: [0, 3] }, ["b"]);
    expect(ids({ output: "artifacts" }, c)).toEqual(["a"]);
    expect(ids({ output: "files" }, c)).toEqual(["d"]);
    expect(ids({ output: "helpers" }, c)).toEqual(["b"]);
  });

  it("match a folder, a chat's own or its project's", () => {
    expect(ids({ folder: "C:\\work\\shop" })).toEqual(["a"]);
    expect(ids({ folder: "C:\\x\\notes" })).toEqual(["d"]);
  });

  it("match a project, or the chats with none", () => {
    expect(ids({ project: "b" })).toEqual(["b"]);
    expect(ids({ project: NO_PROJECT })).toEqual(["c", "d"]);
  });

  it("must all hold together", () => {
    expect(ids({ project: NO_PROJECT, folder: "C:\\x\\notes" })).toEqual(["d"]);
    expect(ids({ project: "s", status: "failed" })).toEqual([]);
  });
});

describe("groups", () => {
  const leaders = [
    { id: "poiesis", name: "Poiesis" },
    { id: "persona:p", name: "Mira" },
  ];
  const labels = (g: { label: string; items: Conversation[] }[]) => g.map((x) => `${x.label}:${x.items.map((i) => i.id).join("")}`);

  it("by status, what needs the person first, and no empty group", () => {
    const states: Record<string, ChatState> = { a: "idle", b: "running", c: "needs-you", d: "idle" };
    const chats = ["a", "b", "c", "d"].map((id) => conv(id));
    expect(labels(groupChats(chats, "status", { ...ctx(states), leaders }))).toEqual([
      "Waiting for you:c",
      "Running:b",
      "Everything else:ad",
    ]);
  });

  it("by leader, in the order of the list, a gone leader read as Poiesis", () => {
    const chats = [conv("a", { leader: "persona:p" }), conv("b"), conv("c", { leader: "persona:gone" })];
    expect(labels(groupChats(chats, "leader", { ...ctx(), leaders }))).toEqual(["Poiesis:bc", "Mira:a"]);
  });

  it("by folder, by name, the chats with none last", () => {
    const chats = [conv("a"), conv("b", { projectId: "s" }), conv("c", { folderPath: "C:\\x\\alpha" })];
    expect(labels(groupChats(chats, "folder", { ...ctx(), leaders }))).toEqual(["alpha:c", "shop:b", "No folder:a"]);
  });
});
