/**
 * @vitest-environment jsdom
 *
 * The steps open while the work is happening and fold to one line once it is
 * done — and a choice the user made by clicking outlasts either state.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Timeline, { summarize } from "./Timeline";
import type { AgentStep } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function step(id: string, verb: string, overrides: Partial<AgentStep> = {}): AgentStep {
  return { id, verb, target: `target ${id}`, status: "done", ...overrides } as AgentStep;
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

const rows = () => container.querySelectorAll('[role="listitem"]').length;
const head = () => container.querySelector<HTMLButtonElement>(".timeline-head")!;

describe("summarize", () => {
  it("counts verbs in the order they first happened, three named at most", () => {
    const steps = [
      step("1", "searched"),
      step("2", "fetched"),
      step("3", "searched"),
      step("4", "created"),
      step("5", "read"),
      step("6", "searched"),
      step("7", "fetched"),
    ];
    // "created" and "read" tie at one; the first to appear keeps its place.
    expect(summarize(steps)).toBe("searched 3 · fetched 2 · created 1 · 1 more");
  });
});

describe("Timeline folding", () => {
  const steps = [step("1", "searched"), step("2", "fetched")];

  it("is closed while live, with the activity in its head, and folds to its summary when the run ends", () => {
    act(() => root.render(<Timeline steps={[...steps, step("3", "read", { status: "running" })]} live />));
    expect(rows()).toBe(0);
    expect(head().getAttribute("aria-expanded")).toBe("false");
    expect(head().textContent).toContain("Working");
    expect(head().textContent).toContain("read target 3");
    // The person's own choice wins: open, it shows the rows.
    act(() => head().click());
    expect(rows()).toBe(3);

    act(() => root.render(<Timeline steps={steps} live={false} />));
    expect(head().textContent).toContain("2 steps");
    expect(head().textContent).toContain("searched 1 · fetched 1");
  });

  it("says how long a finished run took, from the first start to the last end, and each step its own time", () => {
    const timed = [
      step("1", "searched", { startedAt: 1_000, endedAt: 1_400 }),
      step("2", "fetched", { startedAt: 1_400, endedAt: 39_400 }),
      step("3", "read", { startedAt: 39_400, endedAt: 72_000 }),
    ];
    act(() => root.render(<Timeline steps={timed} />));
    expect(head().textContent).toBe("3 steps71s · searched 1 · fetched 1 · read 1".replace("71s", "1m 11s"));
    act(() => head().click());
    expect(Array.from(container.querySelectorAll(".step-time")).map((t) => t.textContent)).toEqual(["0.4s", "38s", "33s"]);
  });

  it("leaves the duration out when a step has no times, and shows no time for a step still running", () => {
    act(() => root.render(<Timeline steps={[step("1", "searched", { startedAt: 1, endedAt: 2 }), step("2", "read")]} />));
    expect(head().textContent).toBe("2 steps" + "searched 1 · read 1");
    act(() => head().click());
    expect(container.querySelectorAll(".step-time")).toHaveLength(1);
    act(() => root.render(<Timeline steps={[step("1", "read", { startedAt: 1, status: "running" })]} live />));
    act(() => head().click());
    expect(container.querySelectorAll(".step-time")).toHaveLength(0);
  });

  it("keeps a finished run open once the user opens it", () => {
    act(() => root.render(<Timeline steps={steps} />));
    act(() => head().click());
    expect(rows()).toBe(2);
    act(() => root.render(<Timeline steps={[...steps, step("3", "read")]} />));
    expect(rows()).toBe(3);
  });

  it("shows only the newest rows of a long live run until asked for the rest", () => {
    const many = Array.from({ length: 10 }, (_, i) => step(String(i), "read"));
    act(() => root.render(<Timeline steps={many} live />));
    act(() => head().click());
    expect(rows()).toBe(6);
    act(() => container.querySelector<HTMLButtonElement>(".timeline-earlier")!.click());
    expect(rows()).toBe(10);
  });
});
