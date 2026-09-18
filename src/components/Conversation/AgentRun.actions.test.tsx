/**
 * @vitest-environment jsdom
 *
 * `HRN-UI-5`: the two actions on a finished turn, and — more importantly — when
 * they are absent.
 *
 * "Continue where I stopped" is the one that can lie. Offered on a turn that
 * finished, it promises to pick up work that is already done. Offered on an
 * older turn, it promises to pick up *that* run when the only run the log can
 * still continue is the last one. Both would be a button that quietly does
 * something other than what it says, so both are pinned here.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
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
  useAppStore.setState({ busy: false });
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

const labels = () =>
  Array.from(container.querySelectorAll(".turn-actions button")).map((b) => b.textContent);

describe("AgentRun — what you can do with a finished turn", () => {
  it("offers a rerun on any finished answer", () => {
    render(message({ streaming: false }));
    expect(labels()).toEqual(["Try again from here"]);
  });

  it("offers to continue only a run that did not finish", () => {
    render(message({ streaming: false, stopReason: "max_steps" }));
    expect(labels()).toContain("Continue where I stopped");

    render(message({ streaming: false, stopReason: "completed" }));
    expect(labels()).not.toContain("Continue where I stopped");
  });

  it("offers to continue only the last turn", () => {
    render(message({ streaming: false, stopReason: "aborted" }), false);
    expect(labels()).not.toContain("Continue where I stopped");
  });

  it("keeps out of the way while anything is running", () => {
    render(message({ streaming: true }));
    expect(labels()).toEqual([]);

    act(() => {
      useAppStore.setState({ busy: true });
    });
    render(message({ streaming: false, stopReason: "aborted" }));
    expect(labels()).toEqual([]);
  });

  it("stays away from a turn that was never written down", () => {
    render(message({ id: "a-1699999999", streaming: false }));
    expect(labels()).toEqual([]);
  });
});
