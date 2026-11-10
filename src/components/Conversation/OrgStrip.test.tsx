/**
 * @vitest-environment jsdom
 *
 * `WRK-9`: the organization as one line above an answer worked item by item.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import OrgStrip from "./OrgStrip";
import { useAppStore } from "../../lib/store";
import type { PlanView } from "../../lib/api";
import type { SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  container.className = "agent-run";
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    activeConversationId: "c1",
    subRuns: {},
    presets: [{ slug: "lean", name: "Lean", line: "x", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] }],
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
    { text: "Find the data", status: "done", review: { state: "checked" } },
    { text: "Export function", status: "doing", review: { state: "checking" } },
    { text: "Button", status: "todo" },
    { text: "Docs", status: "dropped", why: "not needed" },
  ],
};

const dots = () => [...container.querySelectorAll(".org-dot")].map((d) => d.textContent);
const helpers = (n: number) =>
  Object.fromEntries(
    Array.from({ length: n }, (_, i) => [`h${i}`, { status: "running", parentConversationId: "c1" }])
  ) as unknown as Record<string, SubRun>;

describe("the organization strip", () => {
  it("is the preset, then the items as dots on one belt", () => {
    act(() => root.render(<OrgStrip plan={plan} live />));
    expect(container.querySelector(".org-name")?.textContent).toBe("Lean");
    expect(dots()).toEqual(["●", "◔", "○", "×"]);
    expect(container.querySelector(".org-strip")?.className).toContain("live");
    expect(container.querySelector(".org-item.doing")?.getAttribute("title")).toBe("Export function: being checked");
  });

  it("says a second look is under way, and a helper is out, in words and no counts", () => {
    useAppStore.setState({ subRuns: helpers(2) });
    act(() => root.render(<OrgStrip plan={plan} live />));
    expect(container.querySelector(".org-who")?.textContent).toBe("◌ helper◌ second look");
  });

  it("settles into a record of the dots as they ended once the run is over, and has no helpers then", () => {
    useAppStore.setState({ subRuns: helpers(1) });
    act(() => root.render(<OrgStrip plan={{ ...plan, items: plan.items.map((i) => ({ ...i, review: undefined })) }} live={false} />));
    expect(container.querySelector(".org-strip")?.className).toContain("settled");
    expect(container.querySelector(".org-who")?.textContent).toBe("");
  });

  it("is not there for Ad hoc, or for a plan with no items", () => {
    act(() => root.render(<OrgStrip plan={{ ...plan, style: null }} live />));
    expect(container.querySelector(".org-strip")).toBeNull();
    act(() => root.render(<OrgStrip plan={{ ...plan, style: "ad-hoc" }} live />));
    expect(container.querySelector(".org-strip")).toBeNull();
    act(() => root.render(<OrgStrip plan={{ ...plan, items: [] }} live />));
    expect(container.querySelector(".org-strip")).toBeNull();
  });

  it("opens the Coordination view on a click", () => {
    const openOrganization = vi.fn();
    useAppStore.setState({ openOrganization } as never);
    act(() => root.render(<OrgStrip plan={plan} live />));
    act(() => container.querySelector<HTMLButtonElement>(".org-strip")!.click());
    expect(openOrganization).toHaveBeenCalled();
  });
});

describe("the arrangement follows the way of working (WRK-12, WRK-14)", () => {
  const presets = [
    { slug: "command-and-control", name: "Command & Control", line: "x", glyph: "command-and-control", built_in: true, locked: false, version: 1, rules: [] },
    { slug: "mission-command", name: "Mission Command", line: "x", glyph: "mission-command", built_in: true, locked: false, version: 1, rules: [] },
    { slug: "swarm", name: "Swarm", line: "x", glyph: "swarm", built_in: true, locked: false, version: 1, rules: [] },
    { slug: "careful", name: "Careful with mail", line: "x", glyph: "mission-command", built_in: false, locked: true, version: 1, rules: [] },
  ];
  beforeEach(() => useAppStore.setState({ presets }));

  it("Command & Control passes every dot through a node that is you", () => {
    act(() => root.render(<OrgStrip plan={{ ...plan, style: "command-and-control" }} live />));
    expect(container.querySelector(".org-you")?.textContent).toContain("you");
    expect(container.querySelector(".org-gate")).toBeNull();
    expect(dots()).toEqual(["●", "◔", "○", "×"]);
  });

  it("Mission Command puts a gate before done", () => {
    act(() => root.render(<OrgStrip plan={{ ...plan, style: "mission-command" }} live />));
    expect(container.querySelector(".org-gate")?.textContent).toContain("checked");
    expect(container.querySelector(".org-you")).toBeNull();
  });

  it("a goal, which has criteria and no items, is drawn with the criteria as the dots", () => {
    const goal: PlanView = {
      style: "mission-command",
      revisions: 0,
      items: [],
      criteria: [{ text: "npm test exits 0", met: true }, { text: "no new warnings", met: false, why: "two new" }],
    };
    act(() => root.render(<OrgStrip plan={goal} live={false} />));
    expect(dots()).toEqual(["●", "○"]);
    expect(container.querySelector(".org-item.todo")?.getAttribute("title")).toBe("no new warnings: not met");
    // Waiting for the yes, nothing has started, so there is nothing to draw.
    act(() => root.render(<OrgStrip plan={{ ...goal, awaiting_approval: true }} live />));
    expect(container.querySelector(".org-strip")).toBeNull();
  });

  it("Swarm lays the dots side by side and says who has each", () => {
    const swarm: PlanView = {
      style: "swarm",
      revisions: 0,
      items: [
        { text: "Export", status: "doing", by: "helper" },
        { text: "Button", status: "doing", by: "Claude Code" },
        { text: "Docs", status: "todo" },
      ],
    };
    act(() => root.render(<OrgStrip plan={swarm} live />));
    expect(container.querySelector(".org-strip")?.className).toContain("shape-swarm");
    expect([...container.querySelectorAll(".org-by")].map((e) => e.textContent)).toEqual(["helper", "Claude Code"]);
    expect(container.querySelector(".org-item.doing")?.getAttribute("title")).toBe("Export: in hand, helper");
  });

  it("a preset of your own is drawn the way its parent is", () => {
    act(() => root.render(<OrgStrip plan={{ ...plan, style: "careful" }} live />));
    expect(container.querySelector(".org-strip")?.className).toContain("shape-mission-command");
    expect(container.querySelector(".org-name")?.textContent).toBe("Careful with mail");
    expect(container.querySelector(".org-gate")).not.toBeNull();
  });
});
