/**
 * @vitest-environment jsdom
 *
 * `PLN-UI-T1`: the plan card shows the plan the run is actually working to.
 *
 * The failure each of these pins is the same one: a checklist that lies. An
 * item that quietly disappears, a reason that never reaches the screen, a list
 * that was rewritten without saying so — each of them leaves you reading a plan
 * that is not the plan, which is worse than having no plan on screen at all.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import PlanCard from "./PlanCard";
import type { PlanView } from "../../lib/api";

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

function render(plan: PlanView): HTMLElement {
  act(() => {
    root.render(<PlanCard plan={plan} />);
  });
  return container;
}

const base: PlanView = {
  items: [
    { text: "read the spec", status: "done" },
    { text: "sketch the layout", status: "doing" },
    { text: "write the file", status: "todo" },
  ],
  revisions: 0,
};

describe("the plan card", () => {
  it("shows every item with where it stands", () => {
    const el = render(base);
    const rows = [...el.querySelectorAll(".plan-item")];
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("read the spec"),
      expect.stringContaining("sketch the layout"),
      expect.stringContaining("write the file"),
    ]);
    expect(rows.map((r) => r.className)).toEqual([
      expect.stringContaining("done"),
      expect.stringContaining("doing"),
      expect.stringContaining("todo"),
    ]);
    expect(el.querySelector(".plan-progress")?.textContent).toContain("1 of 3 done");
  });

  it("keeps a dropped item on screen, with the reason it was dropped", () => {
    const el = render({
      ...base,
      items: [
        { text: "read the spec", status: "done" },
        { text: "sketch the layout", status: "dropped", why: "the spec already had it" },
      ],
    });
    expect(el.textContent).toContain("sketch the layout");
    expect(el.querySelector(".plan-why")?.textContent).toContain("the spec already had it");
    // Dropped work is not outstanding: counting it against the total would make
    // a finished run look abandoned.
    expect(el.querySelector(".plan-progress")?.textContent).toContain("1 of 1 done");
    expect(el.querySelector(".plan-progress")?.textContent).toContain("1 dropped");
  });

  it("marks work that was added after the plan was written", () => {
    const el = render({
      ...base,
      items: [{ text: "fix the test it broke", status: "todo", added: true }],
    });
    expect(el.querySelector(".plan-added")?.textContent).toContain("added later");
  });

  it("says a plan was revised, and keeps the earlier version behind a disclosure", () => {
    const el = render({
      items: [{ text: "start over", status: "todo" }],
      revisions: 1,
      previous: [["read the spec", "sketch the layout"]],
    });
    expect(el.querySelector(".plan-revised")?.textContent).toContain("2nd version");

    const details = el.querySelector(".plan-history") as HTMLDetailsElement;
    expect(details).toBeTruthy();
    // Closed by default: what the plan says now is the thing to read.
    expect(details.open).toBe(false);
    act(() => {
      details.open = true;
      details.dispatchEvent(new Event("toggle"));
    });
    expect(details.textContent).toContain("read the spec");
    expect(details.textContent).toContain("sketch the layout");
  });

  it("says nothing at all about revisions when the plan was never revised", () => {
    const el = render(base);
    expect(el.querySelector(".plan-revised")).toBeNull();
    expect(el.querySelector(".plan-history")).toBeNull();
  });

  it("renders nothing for an empty plan rather than an empty card", () => {
    expect(render({ items: [], revisions: 0 }).textContent).toBe("");
  });
});
