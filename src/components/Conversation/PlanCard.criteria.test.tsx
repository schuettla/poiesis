/**
 * @vitest-environment jsdom
 *
 * `WRK-10`, `WRK-14`: what done means is agreed before anything changes, on the same
 * card and with the same two buttons as an approved plan. The final check ticks the
 * boxes; one that was not met stays open with its reason. In a swarm each item says who
 * has it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PlanCard from "./PlanCard";
import { useAppStore } from "../../lib/store";
import type { PlanView } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const approvePlan = vi.fn();
const revisePlan = vi.fn();

beforeEach(() => {
  approvePlan.mockReset();
  revisePlan.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ presets: [], approvePlan, revisePlan } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const waiting: PlanView = {
  revisions: 0,
  awaiting_approval: true,
  style: "mission-command",
  criteria: [{ text: "The Fleet view has an Export button" }, { text: "The file opens in Excel with all columns" }],
  items: [{ text: "Export function", status: "todo" }],
};

const text = (sel: string) => [...container.querySelectorAll(sel)].map((e) => e.textContent);
const press = (label: string) =>
  act(() => [...container.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!.click());

describe("what done means (WRK-10)", () => {
  it("is asked for before anything changes, with the plan's own two buttons", () => {
    act(() => root.render(<PlanCard plan={waiting} actionable />));
    expect(container.querySelector(".plan-criteria-title")?.textContent).toBe("What done means");
    expect(text(".plan-criterion .plan-text")).toEqual([
      "The Fleet view has an Export button",
      "The file opens in Excel with all columns",
    ]);
    expect(container.querySelector(".plan-approval-text")?.textContent).toContain("what I'll check myself against");
    press("Go ahead");
    expect(approvePlan).toHaveBeenCalledWith(waiting);
    press("Change something");
    expect(revisePlan).toHaveBeenCalled();
  });

  it("is a card even when there are only criteria so far", () => {
    act(() => root.render(<PlanCard plan={{ ...waiting, items: [] }} actionable />));
    expect(container.querySelector(".plan-card")).not.toBeNull();
    expect(container.querySelector(".plan-progress")).toBeNull();
  });

  it("is not asked about after the yes, and does not claim a check that has not been made", () => {
    act(() => root.render(<PlanCard plan={{ ...waiting, awaiting_approval: false }} actionable />));
    expect(container.querySelector(".plan-approval")).toBeNull();
    expect(text(".plan-criterion .plan-mark")).toEqual(["☐", "☐"]);
  });

  it("is ticked by the final check, and one that was not met stays open with its reason", () => {
    const checked: PlanView = {
      ...waiting,
      awaiting_approval: false,
      criteria: [
        { text: "The Fleet view has an Export button", met: true },
        { text: "The file opens in Excel with all columns", met: false, why: "it opens with one column" },
      ],
    };
    act(() => root.render(<PlanCard plan={checked} />));
    expect(text(".plan-criterion .plan-mark")).toEqual(["☑", "☐"]);
    const rows = [...container.querySelectorAll(".plan-criterion")];
    expect(rows[0].className).toContain("met");
    expect(rows[1].textContent).toContain("it opens with one column");
    expect(rows[0].textContent).not.toContain("—");
  });
});

describe("who has an item (WRK-14)", () => {
  it("is said after the item, for a helper and for an agent app", () => {
    const swarm: PlanView = {
      revisions: 0,
      style: "swarm",
      items: [
        { text: "Write the export", status: "doing", by: "helper" },
        { text: "Write the button", status: "doing", by: "Claude Code" },
        { text: "Docs", status: "todo" },
      ],
    };
    act(() => root.render(<PlanCard plan={swarm} />));
    expect(text(".plan-by")).toEqual([" — helper", " — Claude Code"]);
  });
});
