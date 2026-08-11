/**
 * @vitest-environment jsdom
 *
 * A live turn always shows that it's live, and a dead one shows that it's dead.
 *
 * The bug this pins: the working indicator keyed on whether the turn had
 * produced *any* content, so the first tool step switched it off permanently.
 * A browsing run then spent most of its wall-clock time — the model deciding
 * what to do between calls — looking exactly like a hang.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import AgentRun from "./AgentRun";
import type { AgentStep, Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function step(overrides: Partial<AgentStep> = {}): AgentStep {
  return {
    id: "s1",
    verb: "visited",
    target: "orf.at",
    status: "done",
    ...overrides,
  } as AgentStep;
}

function message(overrides: Partial<Message> = {}): Message {
  return {
    id: "m1",
    role: "assistant",
    text: "",
    ...overrides,
  } as Message;
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

const working = () => container.querySelector(".thinking");
const empty = () => container.querySelector(".run-empty");

describe("AgentRun — is it still working?", () => {
  it("shows the indicator before anything has happened", () => {
    render(message({ streaming: true }));
    expect(working()).not.toBeNull();
  });

  it("keeps showing it between tool calls, once steps have finished", () => {
    // The regression: steps exist and none is running, so nothing on screen
    // moves — this is the model thinking about its next move.
    render(message({ streaming: true, steps: [step(), step({ id: "s2", verb: "clicked" })] }));
    expect(working(), "a finished step must not switch the indicator off").not.toBeNull();
  });

  it("names what it's waiting on once a run is under way", () => {
    render(message({ streaming: true, steps: [step()] }));
    expect(container.textContent).toContain("still working");
  });

  it("stays quiet while a step is in flight — that step already pulses", () => {
    render(message({ streaming: true, steps: [step({ status: "running" })] }));
    expect(working()).toBeNull();
  });

  it("stays quiet while prose is arriving — that carries its own caret", () => {
    render(message({ streaming: true, steps: [step()], text: "Here's the first paragraph" }));
    expect(working()).toBeNull();
    expect(container.querySelector(".run-text.streaming")).not.toBeNull();
  });

  it("stops once the turn ends", () => {
    render(message({ streaming: false, steps: [step()], text: "Done." }));
    expect(working()).toBeNull();
    expect(empty()).toBeNull();
  });

  it("says so when a turn ends having produced nothing", () => {
    // What "go on" did: a finished turn with no steps and no text, which was
    // an empty bubble indistinguishable from one still running.
    render(message({ streaming: false }));
    expect(empty(), "a silent turn must not look like a live one").not.toBeNull();
    expect(working()).toBeNull();
  });

  it("does not claim silence when steps ran but no prose came back", () => {
    // Steps are content: the turn did something visible, even if it ended
    // without a sentence. That's a different failure and reads differently.
    render(message({ streaming: false, steps: [step()] }));
    expect(empty()).toBeNull();
  });
});
