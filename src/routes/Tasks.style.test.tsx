/**
 * @vitest-environment jsdom
 *
 * `WRK-18`: a scheduled task has a way of working. Nobody watches it, so the form says
 * that what needs a person is skipped; Ad hoc is stored as nothing.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Tasks from "./Tasks";
import { useAppStore } from "../lib/store";
import type { ScheduledJob } from "../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const createJob = vi.fn(() => Promise.resolve());
const updateJob = vi.fn(() => Promise.resolve());

let container: HTMLDivElement;
let root: Root;

const job = (over: Partial<ScheduledJob> = {}): ScheduledJob => ({
  id: "j1",
  name: "Releases",
  prompt: "Summarise new releases",
  cadence: "daily",
  scope: null,
  enabled: true,
  next_run_at: 4_000_000_000_000,
  last_run_at: null,
  last_result: null,
  built_in: false,
  source_conversation_id: null,
  runs: [],
  ...over,
});

beforeEach(() => {
  createJob.mockClear();
  updateJob.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    scheduledJobs: [],
    runningJob: null,
    digest: null,
    taskDraft: null,
    presets: [
      { slug: "ad-hoc", name: "Ad hoc", line: "Just answer", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
      { slug: "lean", name: "Lean", line: "One thing at a time", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] },
    ],
    createScheduledJob: createJob,
    updateScheduledJob: updateJob,
    refreshScheduler: () => Promise.resolve(),
    dismissDigest: () => Promise.resolve(),
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const click = (label: string) =>
  act(() => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!.click());
const set = (el: HTMLInputElement | HTMLTextAreaElement, value: string) =>
  act(() => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
const fill = () => {
  set(container.querySelector<HTMLInputElement>("input[placeholder='What to call it']")!, "Releases");
  set(container.querySelector("textarea")!, "Summarise new releases");
};

describe("a task's way of working", () => {
  it("is Ad hoc to begin with, and saved as nothing", async () => {
    act(() => root.render(<Tasks />));
    click("+ New task");
    expect(container.textContent).toContain("How I work on it");
    expect(container.textContent).not.toContain("Nobody is there to ask");
    fill();
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ style: null }));
  });

  it("is chosen from the presets, and says what nobody being there means", async () => {
    act(() => root.render(<Tasks />));
    click("+ New task");
    fill();
    click("Ad hoc");
    act(() => (Array.from(document.querySelectorAll<HTMLElement>("[role=option]")).find((o) => o.textContent?.startsWith("Lean")))!.click());
    expect(container.textContent).toContain("Nobody is there to ask");
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ style: "lean" }));
  });

  it("is filled in when a task is edited", async () => {
    useAppStore.setState({ scheduledJobs: [job({ style: "lean" })] } as never);
    act(() => root.render(<Tasks />));
    click("Edit");
    expect(container.querySelector(".select-value")?.textContent).toContain("Lean");
    await act(async () => click("Save changes"));
    expect(updateJob).toHaveBeenLastCalledWith("j1", expect.objectContaining({ style: "lean" }));
  });
});
