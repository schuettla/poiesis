/**
 * @vitest-environment jsdom
 *
 * `HRN-UI-5`: the actions on a finished turn, and — more importantly — when they
 * are absent.
 *
 * "Continue where I stopped" is the one that can lie. Offered on a turn that
 * finished, it promises to pick up work that is already done. Offered on an
 * older turn, it promises to pick up *that* run when the only run the log can
 * still continue is the last one. Both would be a button that quietly does
 * something other than what it says, so both are pinned here.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AgentRun from "./AgentRun";
import { useAppStore } from "../../lib/store";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function message(overrides: Partial<Message> = {}): Message {
  // A persisted id: the optimistic `a-…` of a turn still in flight has nothing
  // on disk to fork from or resume.
  return { id: "msg-1", role: "assistant", text: "Done.", ...overrides } as Message;
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  useAppStore.setState({
    busyChats: {},
    activeConversationId: "c1",
    conversations: [
      {
        id: "c1",
        title: "t",
        updatedAt: 0,
        messages: [{ id: "msg-0", role: "user", text: "Do it", createdAt: 0 }, message()],
      },
    ],
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function render(m: Message, last = true) {
  act(() => {
    root.render(<AgentRun message={m} last={last} />);
  });
}

/** The buttons are icons, so each is known by its name. */
const labels = () =>
  Array.from(container.querySelectorAll(".turn-actions button")).map((b) => b.getAttribute("aria-label"));
/** `Continue` sits on the line that says why the run stopped. */
const continues = () => !!container.querySelector(".run-stopped .run-continue");

describe("AgentRun — what you can do with a finished turn", () => {
  it("offers a rerun on the last finished answer", () => {
    render(message({ streaming: false }));
    expect(labels()).toEqual(["Copy", "Try again", "Rewind", "Fork"]);
  });

  it("offers no rerun further back, only going back or branching", () => {
    render(message({ streaming: false }), false);
    expect(labels()).toEqual(["Copy", "Rewind", "Fork"]);
  });

  it("copies the answer's text", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(message({ streaming: false }));
    const copy = container.querySelector<HTMLButtonElement>('[aria-label="Copy"]')!;
    await act(async () => copy.click());
    expect(writeText).toHaveBeenCalledWith("Done.");
    expect(copy.dataset.tip).toBe("Copied");
  });

  it("says the time, and keeps the model and the full date for the tooltip", () => {
    const model = { name: "Some Model", provenance: "cloud" as const };
    render(message({ streaming: false, model, createdAt: new Date(2020, 10, 27, 11, 59).getTime() }));
    const time = container.querySelector<HTMLElement>(".turn-time")!;
    expect(time.textContent).not.toContain("Some Model");
    expect(time.dataset.tip).toContain("Some Model");
    expect(time.dataset.tip).toContain("2020");
  });

  it("offers to continue only a run that did not finish", () => {
    render(message({ streaming: false, stopReason: "max_steps" }));
    expect(continues()).toBe(true);
    expect(labels(), "said once, on the stopped line").not.toContain("Continue where I stopped");

    render(message({ streaming: false, stopReason: "completed" }));
    expect(continues()).toBe(false);
  });

  it("offers to continue only the last turn", () => {
    render(message({ streaming: false, stopReason: "aborted" }), false);
    expect(continues()).toBe(false);
  });

  it("only lets you copy while anything is running", () => {
    render(message({ streaming: true }));
    expect(labels()).toEqual([]);

    act(() => {
      useAppStore.setState({ activeConversationId: "c1", busyChats: { c1: "other" } });
    });
    render(message({ streaming: false, stopReason: "aborted" }));
    expect(labels()).toEqual(["Copy"]);
    expect(continues()).toBe(false);
  });

  it("only lets you copy a turn that was never written down", () => {
    render(message({ id: "a-1699999999", streaming: false }));
    expect(labels()).toEqual(["Copy"]);
  });
});
