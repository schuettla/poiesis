/**
 * @vitest-environment jsdom
 *
 * `TEAM-20`: a task says who does it. The editor offers Poiesis, the personas and the teams, the
 * choice is saved with the task, and the card says who does it when it is not Poiesis.
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
  name: "Headlines",
  prompt: "Three headlines on X",
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
    personas: [{ id: "p1", name: "Atlas" }],
    teams: [{ slug: "shop-team", name: "Shop team", coordinator: "persona:p1" }],
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

const mount = () => act(() => root.render(<Tasks />));
const click = (label: string) =>
  act(() => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!.click());
const set = (el: HTMLInputElement | HTMLTextAreaElement, value: string) =>
  act(() => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
const choose = async (label: string, option: string) => {
  await act(async () => container.querySelector<HTMLElement>(`[aria-label="${label}"]`)!.click());
  const o = Array.from(container.querySelectorAll<HTMLElement>("[role=option]")).find((x) => x.textContent?.startsWith(option))!;
  await act(async () => o.click());
};
const fill = () => {
  set(container.querySelector<HTMLInputElement>("input[placeholder='What to call it']")!, "Headlines");
  set(container.querySelector("textarea")!, "Three headlines on X");
};

describe("Who does it", () => {
  it("is Poiesis until somebody else is chosen, and saves none for Poiesis", async () => {
    mount();
    click("+ New task");
    expect(container.textContent).toContain("Who does it");
    fill();
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ leader: null }));
  });

  it("offers the personas and the teams and saves the choice", async () => {
    mount();
    click("+ New task");
    fill();
    await choose("Who does it", "Shop team");
    expect(container.textContent).toContain("leaves the question in Waiting for you");
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ leader: "team:shop-team" }));

    click("+ New task");
    fill();
    await choose("Who does it", "Atlas");
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ leader: "persona:p1" }));
  });

  it("is filled in when a task is edited, and a leader that is gone reads as Poiesis", () => {
    useAppStore.setState({ scheduledJobs: [job({ leader: "team:shop-team" })] });
    mount();
    expect(container.textContent).toContain("Done by Shop team");
    click("Edit");
    expect(container.querySelector<HTMLElement>('[aria-label="Who does it"]')?.textContent).toContain("Shop team");
    click("Cancel");

    act(() => useAppStore.setState({ scheduledJobs: [job({ leader: "team:gone" })] }));
    expect(container.textContent).not.toContain("Done by");
    click("Edit");
    expect(container.querySelector<HTMLElement>('[aria-label="Who does it"]')?.textContent).toContain("Poiesis");
  });
});
