/**
 * @vitest-environment jsdom
 *
 * `HRN-3`: a run that stopped short says so.
 *
 * The failure this pins is the one the old loop had: hitting the tool-step cap
 * threw the whole run away and left an error, and a cancelled run returned
 * whatever prose it had with nothing to mark it as partial. Either way the user
 * could not tell "this is my answer" from "this is what I had when I ran out".
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import AgentRun from "./AgentRun";
import type { Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function message(overrides: Partial<Message> = {}): Message {
  return { id: "m1", role: "assistant", text: "", ...overrides } as Message;
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function render(m: Message) {
  act(() => {
    root.render(<AgentRun message={m} />);
  });
}

const stopped = () => container.querySelector(".run-stopped");
const empty = () => container.querySelector(".run-empty");

describe("AgentRun — how a turn ended", () => {
  it("says nothing extra when the model simply finished", () => {
    render(message({ streaming: false, text: "Here you go." }));
    expect(stopped()).toBeNull();
  });

  it("marks an answer that ran out of steps as partial", () => {
    render(message({ streaming: false, text: "Half of it.", stopReason: "max_steps" }));
    expect(container.textContent).toContain("step limit");
    expect(container.textContent).toContain("This is what I had");
  });

  it("distinguishes running out of time from being stopped", () => {
    render(message({ streaming: false, text: "Partway.", stopReason: "timeout" }));
    expect(container.textContent).toContain("ran out of time");

    render(message({ streaming: false, text: "Partway.", stopReason: "aborted" }));
    expect(container.textContent).toContain("You stopped me");
  });

  it("does not also claim the turn said nothing", () => {
    // Two lines of bad news about the same event reads as a broken UI. The
    // stop reason is the better of the two, so it is the one that survives.
    render(message({ streaming: false, stopReason: "aborted" }));
    expect(stopped()).not.toBeNull();
    expect(empty()).toBeNull();
  });

  it("keeps quiet while the turn is still live", () => {
    render(message({ streaming: true, text: "working", stopReason: "aborted" }));
    expect(stopped()).toBeNull();
  });
});
