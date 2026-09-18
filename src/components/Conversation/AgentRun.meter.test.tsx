/**
 * @vitest-environment jsdom
 *
 * The live status line says what the run is doing.
 *
 * The bug this pins: it used to read "step 1 of 12". That is the step *budget*,
 * not a plan and not a status — it told you nothing about what was happening,
 * and the "of 12" read as though the app had worked out twelve steps in
 * advance, which it never does. A run that sat thinking for ten minutes showed
 * the same line as one doing useful work.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import AgentRun from "./AgentRun";
import { useAppStore } from "../../lib/store";
import type { AgentStep, Message } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

function activeRun(overrides: Record<string, unknown> = {}) {
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
      ...overrides,
    },
  } as never);
}

function message(steps?: AgentStep[]): Message {
  return { id: "m1", role: "assistant", text: "", streaming: true, steps } as Message;
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
  useAppStore.setState({ activeRun: null } as never);
});

function meter(m: Message): string {
  act(() => {
    root.render(<AgentRun message={m} />);
  });
  return container.querySelector(".run-meter")?.textContent ?? "";
}

describe("the run meter", () => {
  it("never shows the step budget, which is a setting and not a status", () => {
    activeRun();
    const text = meter(message());
    expect(text).not.toContain("of 12");
    expect(text).not.toMatch(/step \d/);
  });

  it("names the tool that is actually running", () => {
    activeRun();
    expect(meter(message([{ id: "s1", verb: "searched", target: "job market", status: "running" }] as AgentStep[])))
      .toContain("searched job market");
  });

  it("says it is thinking when thinking is what is arriving", () => {
    activeRun({ thinking: "working through it" });
    expect(meter(message())).toContain("thinking");
  });

  it("prefers a running tool over thinking — the tool is the more specific truth", () => {
    activeRun({ thinking: "some leftover" });
    const text = meter(message([{ id: "s1", verb: "read", target: "notes.md", status: "running" }] as AgentStep[]));
    expect(text).toContain("read notes.md");
    expect(text).not.toContain("thinking");
  });

  it("says what it is actually waiting on rather than a vague word", () => {
    activeRun();
    // A finished step and nothing new yet: the gap between steps, which is
    // most of a browsing run's wall clock and used to look like a hang. It used
    // to read "working", which beside a clock at 10:56 says nothing about where
    // the time is going — the loop is sitting on a model call, so say so.
    expect(meter(message([{ id: "s1", verb: "visited", target: "orf.at", status: "done" }] as AgentStep[])))
      .toContain("waiting for the model");
  });
});

/**
 * `PLN-UI-T2`: the meter leads with the plan item and keeps the pulse.
 *
 * This is the payoff for having removed "of 12". The tool line says what the
 * app is doing; the plan item says what the work is *for* — and when one item
 * takes six tool calls, it is the only part of the meter that stays still long
 * enough to read. Staying still is also its danger, which is why it never
 * replaces the activity beside it. Everything falls back exactly as it did
 * before, so a run without a plan is untouched by this.
 */
describe("the run meter, once there is a plan (`PLN-UI-2`)", () => {
  const plan = (status: "todo" | "doing" | "done") => ({
    items: [
      { text: "read the spec", status: "done" as const },
      { text: "sketch the layout", status },
    ],
    revisions: 0,
  });

  it("names the plan item and what is happening under it, not one or the other", () => {
    // The bug this pins: showing only the item made a stalled run look exactly
    // like a working one — the line sat unchanged for as long as an item took,
    // and ten minutes of nothing read the same as ten minutes of work. The item
    // is the frame; the activity is the pulse. Both, or neither is trustworthy.
    activeRun({ plan: plan("doing") });
    const text = meter(
      message([{ id: "s1", verb: "read", target: "spec.md", status: "running" }] as AgentStep[])
    );
    expect(text).toContain("sketch the layout");
    expect(text).toContain("read spec.md");
  });

  it("keeps the meter to one line when the plan item is a paragraph", () => {
    activeRun({
      plan: {
        items: [
          {
            text: "Research key AI-driven marketing shifts (generative search, hyper-personalization, AI agents, synthetic content)",
            status: "doing" as const,
          },
        ],
        revisions: 0,
      },
    });
    const text = meter(message());
    expect(text).toContain("Research key AI-driven marketing shifts");
    expect(text).toContain("…");
    expect(text).not.toContain("synthetic content");
  });

  it("falls back to the running tool when no item is marked as being worked on", () => {
    // A model that writes a plan and then never says which item it is on. The
    // meter must not go quiet about work that is visibly happening.
    activeRun({ plan: plan("todo") });
    expect(
      meter(message([{ id: "s1", verb: "read", target: "spec.md", status: "running" }] as AgentStep[]))
    ).toContain("read spec.md");
  });

  it("falls back exactly as it did before when there is no plan at all", () => {
    activeRun();
    expect(meter(message([{ id: "s1", verb: "read", target: "spec.md", status: "running" }] as AgentStep[])))
      .toContain("read spec.md");
    activeRun({ thinking: "hmm" });
    expect(meter(message())).toContain("thinking");
  });
});
