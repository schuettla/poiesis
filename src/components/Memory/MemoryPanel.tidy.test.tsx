/**
 * @vitest-environment jsdom
 *
 * `SLP-1`: the pending tidy-up shows what the daily pass found, one row each, and
 * `Not this one` takes a row out without touching the rest.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Consolidation } from "../../lib/api";

const calls: string[][] = [];
const pending: Consolidation = {
  deletes: ["old-fact"],
  edits: [],
  merges: [{ keep: "prefers-tea", drop: ["likes-tea"], text: "Prefers tea." }],
  lesson_deletes: ["stale-lesson"],
  promotes: [
    { name: "check-header", description: "Check the header row first", body: "b", evidence: ["c1", "c2", "c3"], into: null },
  ],
  contests: [["prefers-tea", "prefers-coffee"]],
};

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listMemoryFacts: () => Promise.resolve([]),
  listEarlierVersions: () => Promise.resolve([]),
  listUnsure: () => Promise.resolve([]),
  listLetGo: () => Promise.resolve([]),
  listRecentChanges: () => Promise.resolve([]),
  getProfile: () => Promise.resolve(null),
  getPendingConsolidation: () => Promise.resolve(pending),
  dropPendingRow: (kind: string, key: string) => {
    calls.push([kind, key]);
    return Promise.resolve({ ...pending, promotes: [] });
  },
}));

import { useAppStore } from "../../lib/store";
import MemoryPanel from "./MemoryPanel";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const flush = () => act(async () => void (await Promise.resolve()));

beforeEach(() => {
  calls.length = 0;
  useAppStore.setState({
    refreshMemoryContext: () => Promise.resolve(),
    refreshChangeProposals: () => Promise.resolve(),
    conversations: [],
  } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the pending tidy-up", () => {
  it("shows every kind of row the daily pass can find, each with Not this one", async () => {
    act(() => root.render(<MemoryPanel />));
    await flush();
    await flush();
    const text = container.textContent ?? "";
    expect(text).toContain("I saw this in 3 conversations and would keep it");
    expect(text).toContain("Check the header row first");
    expect(text).toContain("keep prefers-tea, fold in likes-tea");
    expect(text).toContain("These cannot all be true: prefers-tea, prefers-coffee");
    expect(text).toContain("old-fact");
    expect(text).toContain("the lesson stale-lesson");
    expect([...container.querySelectorAll("button")].filter((b) => b.textContent === "Not this one")).toHaveLength(5);
  });

  it("takes one row out and keeps the rest", async () => {
    act(() => root.render(<MemoryPanel />));
    await flush();
    await flush();
    const first = [...container.querySelectorAll("button")].find((b) => b.textContent === "Not this one")!;
    await act(async () => void first.click());
    expect(calls).toEqual([["promote", "check-header"]]);
    expect(container.textContent).not.toContain("would keep it");
    expect(container.textContent).toContain("the lesson stale-lesson");
  });
});
