/**
 * @vitest-environment jsdom
 *
 * `UCM-8`: a draft from `/schedule` opens the editor filled in. A rhythm I could
 * read is chosen and any difference from what was asked is said; one I could not
 * read is left empty and focused, and nothing can be saved until the user picks.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Tasks from "./Tasks";
import { useAppStore } from "../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const createJob = vi.fn(() => Promise.resolve());

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  createJob.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    scheduledJobs: [],
    runningJob: null,
    digest: null,
    taskDraft: null,
    createScheduledJob: createJob,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function open(draft: NonNullable<ReturnType<typeof useAppStore.getState>["taskDraft"]>) {
  useAppStore.setState({ taskDraft: draft });
  act(() => root.render(<Tasks />));
}

const pressed = () =>
  Array.from(container.querySelectorAll<HTMLButtonElement>(".self-segment"))
    .filter((b) => b.getAttribute("aria-pressed") === "true")
    .map((b) => b.textContent);
const save = () => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === "Create task")!;
const text = () => container.textContent ?? "";

describe("a draft from /schedule", () => {
  it("opens with the rhythm chosen, the task filled in, and nothing saved", () => {
    open({ name: "Chat", prompt: "sync my notes", conversationId: "c1", cadence: "six-hourly" });
    expect(pressed()).toEqual(["Every 6 hours"]);
    expect(container.querySelector("textarea")?.value).toBe("sync my notes");
    expect(save().disabled).toBe(false);
    expect(createJob).not.toHaveBeenCalled();
  });

  it("says where my reading differs from what was asked", () => {
    open({
      name: "Chat",
      prompt: "summarise my mail",
      conversationId: "c1",
      cadence: "daily",
      whenNote: "I can't skip weekends yet, so I'll run it every day.",
    });
    expect(text()).toContain("I can't skip weekends yet");
    expect(pressed()).toEqual(["Daily"]);
  });

  it("with no rhythm read, leaves it empty, focuses it, and will not save until one is chosen", () => {
    open({ name: "Chat", prompt: "summarise my mail", conversationId: "c1", cadence: null });
    expect(pressed()).toEqual([]);
    expect(document.activeElement?.className).toContain("self-segment");
    expect(text()).toContain("Choose how often");
    expect(save().disabled).toBe(true);

    const weekly = Array.from(container.querySelectorAll<HTMLButtonElement>(".self-segment")).find(
      (b) => b.textContent === "Weekly"
    )!;
    act(() => weekly.click());
    expect(pressed()).toEqual(["Weekly"]);
    expect(save().disabled).toBe(false);
  });

  it("saves only when asked, with the rhythm that was chosen", async () => {
    open({ name: "Chat", prompt: "sync", conversationId: "c1", cadence: "hourly" });
    await act(async () => save().click());
    expect(createJob).toHaveBeenCalledWith(
      expect.objectContaining({ prompt: "sync", cadence: "hourly", source_conversation_id: "c1" })
    );
  });

  it("a draft without an opinion keeps the old default (Schedule this)", () => {
    open({ name: "Chat", prompt: "from the chat", conversationId: "c1" });
    expect(pressed()).toEqual(["Daily"]);
  });
});
