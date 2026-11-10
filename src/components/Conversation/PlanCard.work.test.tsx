/**
 * @vitest-environment jsdom
 *
 * `WRK-1`, `WRK-2`: the board of a run that works a way. What done means is under
 * each open item, the preset is in the head, and while a check fails the card says
 * so once, at the top.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import PlanCard from "./PlanCard";
import Timeline from "./Timeline";
import { useAppStore } from "../../lib/store";
import type { PlanView } from "../../lib/api";
import type { AgentStep } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    presets: [
      { slug: "lean", name: "Lean", line: "x", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] },
    ],
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const plan: PlanView = {
  style: "lean",
  revisions: 0,
  items: [
    { text: "Export function", status: "done", done_when: "the function writes a file" },
    { text: "Button in the Fleet view", status: "doing", done_when: "the button is there and clickable" },
    { text: "Docs", status: "todo", done_when: "the readme mentions it" },
    { text: "Old idea", status: "dropped", why: "not needed", done_when: "never" },
  ],
};

describe("the board", () => {
  it("says what done means under each open item, and not under finished or dropped ones", () => {
    act(() => root.render(<PlanCard plan={plan} />));
    const rows = [...container.querySelectorAll(".plan-item")];
    expect(rows[0].querySelector(".plan-done-when")?.textContent).toBe("Done when: the function writes a file");
    expect(rows[1].querySelector(".plan-done-when")?.textContent).toBe("Done when: the button is there and clickable");
    expect(rows[2].querySelector(".plan-done-when")?.textContent).toBe("Done when: the readme mentions it");
    // Hidden by the stylesheet for the two that are over; the words stay in the markup for a reader.
    expect(rows[0].className).toContain("done");
    expect(rows[3].className).toContain("dropped");
  });

  it("draws the preset's organization in the head, and nothing for Ad hoc", () => {
    act(() => root.render(<PlanCard plan={plan} />));
    expect(container.querySelector(".plan-head .org-strip .org-name")?.textContent).toBe("Lean");
    expect(container.querySelectorAll(".plan-head .org-dot")).toHaveLength(plan.items.length);
    act(() => root.render(<PlanCard plan={{ ...plan, style: undefined }} />));
    expect(container.querySelector(".plan-style")).toBeNull();
    expect(container.querySelector(".org-strip")).toBeNull();
  });

  it("shows a band at the top while the line is stopped, in the first person", () => {
    act(() => root.render(<PlanCard plan={plan} stopped="cargo test" />));
    const band = container.querySelector(".plan-stopped");
    expect(band?.textContent).toBe("Stopped: cargo test is failing. I'm fixing that before anything else.");
    expect(container.querySelector(".plan-card")?.firstElementChild).toBe(band);
    act(() => root.render(<PlanCard plan={plan} />));
    expect(container.querySelector(".plan-stopped")).toBeNull();
  });
});

describe("the timeline", () => {
  const steps: AgentStep[] = [
    { id: "a", verb: "read", target: "a.ts", status: "done" },
    { id: "item-0", verb: "", target: "", status: "done", divider: "Done: Export function" },
    { id: "b", verb: "edited", target: "b.ts", status: "done" },
  ];

  it("draws a thin line where an item ended, and does not count it as a step", () => {
    act(() => root.render(<Timeline steps={steps} live />));
    expect(container.querySelector(".timeline-summary")?.textContent).toBe("2 steps");
    const rows = [...container.querySelectorAll(".timeline-rows > *")];
    expect(rows.map((r) => r.className.split(" ")[0])).toEqual(["step", "item-divider", "step"]);
    expect(container.querySelector(".item-divider")?.textContent).toBe("Done: Export function");
  });

  it("keeps the divider out of the one-line summary of a finished run", () => {
    act(() => root.render(<Timeline steps={steps} />));
    expect(container.querySelector(".timeline-summary")?.textContent).toBe("read 1 · edited 1");
  });
});
