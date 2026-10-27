/**
 * @vitest-environment jsdom
 *
 * `HAB-UI-1`: a lesson that spoke up at a tool step is one plain line in that step's
 * expanded detail. Nothing about it shows until the step is opened.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Timeline from "./Timeline";
import type { AgentStep } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

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

describe("a lesson that fired", () => {
  it("is one plain line inside the step's detail, and not before it is opened", () => {
    const step: AgentStep = {
      id: "s1",
      verb: "read",
      target: "data.csv",
      status: "done",
      fired: [{ lesson: "check-header", line: "Checked the header row. You showed me on 4 Oct." }],
    };
    act(() => root.render(<Timeline steps={[step]} live />));
    expect(container.textContent).not.toContain("You showed me on 4 Oct");
    const open = container.querySelector<HTMLButtonElement>("button[aria-label='Show what I checked here']");
    expect(open).not.toBeNull();
    act(() => open!.click());
    expect(container.querySelector(".fired-disclosure")?.textContent).toBe(
      "Checked the header row. You showed me on 4 Oct."
    );
  });

  it("adds no row of its own to a step that fired nothing", () => {
    const step: AgentStep = { id: "s2", verb: "read", target: "a.md", status: "done" };
    act(() => root.render(<Timeline steps={[step]} live />));
    expect(container.querySelector(".fired-disclosure")).toBeNull();
    expect(container.querySelector(".step-expand")).toBeNull();
  });
});
