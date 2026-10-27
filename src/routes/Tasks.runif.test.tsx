/**
 * @vitest-environment jsdom
 *
 * `USE-UI-6`: a task may carry an "only run if" condition. The editor takes it,
 * the card says it, and a run the condition skipped is a muted row in the history
 * with the probability and no chat to open.
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
const setActiveConversation = vi.fn();

let container: HTMLDivElement;
let root: Root;

const job = (over: Partial<ScheduledJob> = {}): ScheduledJob => ({
  id: "j1",
  name: "Releases",
  prompt: "Summarise new releases of https://github.com/acme/widgets/releases",
  cadence: "daily",
  scope: null,
  enabled: true,
  next_run_at: 4_000_000_000_000,
  last_run_at: 1_700_000_000_000,
  last_result: "Found v2.3.1",
  built_in: false,
  source_conversation_id: null,
  runs: [],
  ...over,
});

beforeEach(() => {
  createJob.mockClear();
  updateJob.mockClear();
  setActiveConversation.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    scheduledJobs: [],
    runningJob: null,
    digest: null,
    taskDraft: null,
    createScheduledJob: createJob,
    updateScheduledJob: updateJob,
    refreshScheduler: () => Promise.resolve(),
    dismissDigest: () => Promise.resolve(),
    setActiveConversation,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const mount = () => act(() => root.render(<Tasks />));
const click = (label: string) =>
  act(() => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!.click());
const field = () => Array.from(container.querySelectorAll<HTMLInputElement>("input[type=text]")).find((i) => i.placeholder.startsWith("there's a new release"));

describe("the Only run if field", () => {
  it("is in the editor, empty and optional, with no explanation until it has a condition", () => {
    mount();
    click("+ New task");
    expect(container.textContent).toContain("Only run if… (optional)");
    expect(field()?.value).toBe("");
    expect(container.textContent).not.toContain("Checked on this computer");
  });

  it("saves the condition with the task, and null when it is left blank", async () => {
    mount();
    click("+ New task");
    const set = (el: HTMLInputElement | HTMLTextAreaElement, value: string) =>
      act(() => {
        const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
    set(container.querySelector<HTMLInputElement>("input[placeholder='What to call it']")!, "Releases");
    set(container.querySelector("textarea")!, "Summarise new releases");
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ run_if: null }));

    click("+ New task");
    set(container.querySelector<HTMLInputElement>("input[placeholder='What to call it']")!, "Releases");
    set(container.querySelector("textarea")!, "Summarise new releases");
    set(field()!, "  there is a new release since last time ");
    expect(container.textContent).toContain("Checked on this computer before each scheduled run");
    expect(container.textContent).toContain("Fast & slow thinking");
    await act(async () => click("Create task"));
    expect(createJob).toHaveBeenLastCalledWith(expect.objectContaining({ run_if: "there is a new release since last time" }));
  });

  it("is filled in when a task is edited", () => {
    useAppStore.setState({ scheduledJobs: [job({ run_if: "there is a new release" })] });
    mount();
    click("Edit");
    expect(field()?.value).toBe("there is a new release");
  });
});

describe("a task with a condition", () => {
  it("says on its card that it only runs if", () => {
    useAppStore.setState({ scheduledJobs: [job({ run_if: "there is a new release" })] });
    mount();
    expect(container.textContent).toContain("Only runs if there is a new release");
    useAppStore.setState({ scheduledJobs: [job()] });
    mount();
    expect(container.textContent).not.toContain("Only runs if");
  });

  it("lists a skipped run as a muted row with the probability, and nothing to open", () => {
    useAppStore.setState({
      scheduledJobs: [
        job({
          run_if: "there is a new release",
          runs: [
            { conversation_id: "", at: 1_700_000_200_000, summary: "Skipped: the condition did not hold", skipped: 0.07 },
            { conversation_id: "c9", at: 1_700_000_000_000, summary: "Found v2.3.1" },
          ],
        }),
      ],
    });
    mount();
    click("2 runs");
    const skipped = container.querySelector(".task-run-skipped");
    expect(skipped?.textContent).toContain("Skipped: the condition did not hold");
    expect(skipped?.querySelector(".task-run-summary")?.textContent).not.toMatch(/\d\.\d/);
    expect(skipped?.querySelector(".task-run-summary")?.getAttribute("title")).toBe(
      "Quick decision: the chance that the condition held was 0.07"
    );
    expect(skipped?.querySelector("button"), "a skip has no chat").toBeNull();
    // The real run beside it is still a button that opens its chat.
    const open = container.querySelector<HTMLButtonElement>(".task-run-open")!;
    act(() => open.click());
    expect(setActiveConversation).toHaveBeenCalledWith("c9");
    expect(container.querySelectorAll(".task-run-open")).toHaveLength(1);
  });
});
