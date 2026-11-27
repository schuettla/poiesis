/**
 * @vitest-environment jsdom
 *
 * The moving words of a live turn: one light per turn, on the line that says what is
 * happening now, and a thinking count that steps up in tokens.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AgentRun from "./AgentRun";
import { ROLL_STEP_MS } from "../Ticker/RollingNumber";
import { useAppStore } from "../../lib/store";
import type { AgentStep, Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function activeRun(overrides: Record<string, unknown> = {}) {
  useAppStore.setState({
    activeConversationId: "c1",
    liveRuns: { c1: {
      runId: "r1",
      convId: "c1",
      step: 1,
      maxSteps: 12,
      startedAt: Date.now(),
      contextTokens: 0,
      contextWindow: null,
      thinking: "",
      ...overrides,
    } },
  } as never);
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ liveRuns: {}, pendingQuestion: null, pendingPermissions: [] } as never);
  vi.useRealTimers();
});

function render(steps?: AgentStep[]) {
  const message = { id: "m1", role: "assistant", text: "", streaming: true, steps } as Message;
  act(() => root.render(<AgentRun message={message} />));
}

const lit = () => [...container.querySelectorAll(".live-words")].map((e) => e.textContent);
const count = () => container.querySelector(".run-thinking-count .roll")?.getAttribute("aria-label");
const running: AgentStep[] = [{ id: "s1", verb: "searched", target: "job market", status: "running" } as AgentStep];

describe("the moving words of a live turn", () => {
  it("lights the working line before any thought arrives", () => {
    activeRun();
    render();
    expect(lit()).toEqual(["waiting for the model"]);
  });

  it("gives the light to Thinking while a thought is arriving", () => {
    activeRun({ thinking: "working through it" });
    render();
    expect(lit()).toEqual(["Thinking"]);
  });

  it("gives it to Working under a timeline, and to Thinking once only a thought moves", () => {
    activeRun({ thinking: "left from the last call" });
    render(running);
    expect(lit()).toEqual(["Working"]);
    render([{ ...running[0], status: "done" }]);
    expect(lit()).toEqual(["Thinking"]);
  });

  it("stands still while the run waits for you", () => {
    activeRun();
    useAppStore.setState({ pendingQuestion: { runId: "r1", messageId: "other" } } as never);
    render(running);
    expect(lit()).toEqual([]);
  });

  it("stands still while a permission is asked for, on the head and the row", () => {
    activeRun();
    useAppStore.setState({ pendingPermissions: [{ id: "p1" }], permissionChats: { p1: "c1" } } as never);
    render(running);
    expect(lit()).toEqual([]);
    act(() => container.querySelector<HTMLButtonElement>(".timeline-head")!.click());
    expect(lit()).toEqual([]);
  });

  it("moves it to the running step's row when the timeline is opened", () => {
    activeRun();
    render(running);
    act(() => container.querySelector<HTMLButtonElement>(".timeline-head")!.click());
    expect(lit()).toEqual(["searched"]);
  });

  it("flaps in the count of a run that ended in front of you, not of one read back", () => {
    const done: AgentStep[] = [{ ...running[0], status: "done" }];
    const finished = { id: "m1", role: "assistant", text: "Done.", steps: done } as Message;
    act(() => root.render(<AgentRun message={finished} />));
    expect(container.querySelector(".timeline.arrived")).toBeNull();

    act(() => root.unmount());
    root = createRoot(container);
    activeRun();
    render(running);
    act(() => root.render(<AgentRun message={finished} />));
    expect(container.querySelector(".timeline.settled.arrived")).not.toBeNull();
  });

  it("counts the thought in estimated tokens, in steps", () => {
    activeRun({ thinking: "a".repeat(40) });
    render();
    expect(count()).toBe("10");
    act(() => activeRun({ thinking: "a".repeat(4800) }));
    expect(count(), "not on every chunk").toBe("10");
    act(() => vi.advanceTimersByTime(ROLL_STEP_MS));
    expect(count()).toBe((1200).toLocaleString());
  });
});
