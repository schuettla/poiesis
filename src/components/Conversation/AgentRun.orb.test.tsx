/**
 * @vitest-environment jsdom
 *
 * A live turn shows exactly one orb, and it says what the turn is doing.
 *
 * Two animations stacked in one turn would break the rule that only one slow
 * animation is on screen at a time, and an orb that disagreed with the status
 * line beside it would be worse than none. The canvas has no state in the DOM,
 * so the library is stood in for by a span that carries it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("thinking-orbs", () => ({
  ThinkingOrb: ({ state, size }: { state: string; size: number }) => (
    <span data-orb={state} data-size={size} />
  ),
}));

import AgentRun from "./AgentRun";
import { useAppStore } from "../../lib/store";
import type { AgentStep, Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const step = (verb: string, status: AgentStep["status"]): AgentStep =>
  ({ id: verb, verb, target: "x", status }) as AgentStep;

function message(overrides: Partial<Message> = {}): Message {
  return { id: "m1", role: "assistant", text: "", streaming: true, ...overrides } as Message;
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeRun: {
      runId: "r1",
      convId: "c1",
      step: 1,
      maxSteps: 12,
      startedAt: Date.now(),
      contextTokens: 0,
      contextWindow: null,
      thinking: "",
    },
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ activeRun: null } as never);
});

function orbs(m: Message): string[] {
  act(() => {
    root.render(<AgentRun message={m} />);
  });
  return [...container.querySelectorAll("[data-orb]")].map((n) => n.getAttribute("data-orb")!);
}

describe("AgentRun — the activity orb", () => {
  it("shows the working orb while the model decides what to do next", () => {
    expect(orbs(message({ steps: [step("visited", "done")] }))).toEqual(["working"]);
  });

  it("follows the step that is running, with no second orb from the thinking line", () => {
    expect(orbs(message({ steps: [step("searched", "running")] }))).toEqual(["searching"]);
    expect(orbs(message({ steps: [step("worked out", "running")] }))).toEqual(["solving"]);
  });

  it("composes while prose is arriving", () => {
    expect(orbs(message({ text: "Here is" }))).toEqual(["composing"]);
  });

  it("shows no orb once the turn has finished", () => {
    expect(orbs(message({ streaming: false, text: "Done.", steps: [step("read", "done")] }))).toEqual([]);
  });
});
