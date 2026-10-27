/**
 * @vitest-environment jsdom
 *
 * `SLP-1`, `SLP-3`: the built-in job says what it is (the daily tidy-up), and its digest
 * offers Show me, and Undo only when the pass changed things on its own.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: string[] = [];
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  undoLastTidy: () => {
    calls.push("undo");
    return Promise.resolve();
  },
}));

import Tasks from "./Tasks";
import { useAppStore } from "../lib/store";
import type { Digest, ScheduledJob } from "../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const openSelf = vi.fn();

const builtIn: ScheduledJob = {
  id: "nightly-reflection",
  name: "Daily tidy-up",
  prompt: "",
  cadence: "daily",
  scope: null,
  enabled: true,
  next_run_at: 4_000_000_000_000,
  last_run_at: null,
  last_result: null,
  built_in: true,
  source_conversation_id: null,
  runs: [],
};

const digest = (over: Partial<Digest> = {}): Digest => ({
  text: "While you were away I read back over 3 conversations. I learned one thing, merged 2 notes, and let one old one go.",
  created_at: 1,
  unread: false,
  ...over,
});

function mount(d: Digest | null) {
  useAppStore.setState({
    scheduledJobs: [builtIn],
    runningJob: null,
    digest: d,
    taskDraft: null,
    refreshScheduler: () => Promise.resolve(),
    dismissDigest: () => Promise.resolve(),
    refreshMemoryContext: () => Promise.resolve(),
    openSelf,
  } as never);
  act(() => root.render(<Tasks />));
}
const button = (label: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === label);

beforeEach(() => {
  calls.length = 0;
  openSelf.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the daily tidy-up", () => {
  it("is named for what it does, in true words", () => {
    mount(null);
    expect(container.textContent).toContain("Daily tidy-up");
    expect(container.textContent).toContain("while you're not using me");
    expect(container.textContent).toContain("Do this once a day");
    expect(container.textContent).not.toContain("Nightly");
  });

  it("offers Show me, which opens the Memory tab, and no Undo when nothing was applied", () => {
    mount(digest());
    act(() => void button("Show me")!.click());
    expect(openSelf).toHaveBeenCalledWith("memory");
    expect(button("Undo")).toBeUndefined();
  });

  it("offers Undo when the pass applied changes, and uses it", async () => {
    mount(digest({ undoable: true }));
    await act(async () => void button("Undo")!.click());
    expect(calls).toEqual(["undo"]);
  });
});
