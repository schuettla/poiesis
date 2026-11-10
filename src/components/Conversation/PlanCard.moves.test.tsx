/**
 * @vitest-environment jsdom
 *
 * `WRK-4`, `WRK-5`, `WRK-6`, `WRK-7`: what the board shows of the second look, of
 * two ways, and of why a move happened. A verdict that sends an item back always
 * shows its reason; ⓘ says the rule behind a move; "Not this time" reaches the run.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const skipMove = vi.fn();
const branchView = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  skipMove: (...a: unknown[]) => skipMove(...a),
  branchView: (...a: unknown[]) => branchView(...a),
}));

import PlanCard from "./PlanCard";
import Timeline from "./Timeline";
import { useAppStore } from "../../lib/store";
import type { PlanView } from "../../lib/api";
import type { AgentStep } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  skipMove.mockReset().mockResolvedValue(true);
  branchView.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    presets: [{ slug: "lean", name: "Lean", line: "x", glyph: "lean", built_in: true, locked: false, version: 1, rules: [] }],
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const rows = () => [...container.querySelectorAll<HTMLElement>(".plan-item")];

describe("the second look on the board", () => {
  const plan = (review: NonNullable<PlanView["items"][number]["review"]>, status: "doing" | "done" = "doing"): PlanView => ({
    style: "lean",
    revisions: 0,
    items: [{ text: "Export function", status, done_when: "it writes a file", review }],
  });

  it("marks an item being looked at with its own mark and says so", () => {
    act(() => root.render(<PlanCard plan={plan({ state: "checking" })} />));
    const row = rows()[0];
    expect(row.querySelector(".plan-mark")?.textContent).toBe("◔");
    expect(row.querySelector(".plan-mark")?.getAttribute("aria-label")).toBe("second look:");
    expect(row.querySelector(".plan-review")?.textContent).toBe("Second look…");
    expect(row.className).toContain("checking");
  });

  it("shows a send-back with its reason, and the item back in hand", () => {
    act(() => root.render(<PlanCard plan={plan({ state: "sent_back", reason: "Nothing shows it runs." })} />));
    const row = rows()[0];
    expect(row.querySelector(".plan-mark")?.textContent).toBe("↺");
    expect(row.querySelector(".plan-review")?.textContent).toBe("Sent back: Nothing shows it runs.");
  });

  it("says Checked on an item the look agreed with, with the reason on hover", () => {
    act(() => root.render(<PlanCard plan={plan({ state: "checked", reason: "It was run." }, "done")} />));
    const note = rows()[0].querySelector<HTMLElement>(".plan-review")!;
    expect(note.textContent).toBe("Checked");
    expect(note.title).toBe("It was run.");
    expect(rows()[0].querySelector(".plan-mark")?.textContent).toBe("☑");
  });
});

describe("two ways on the board", () => {
  const base: PlanView = {
    style: "lean",
    revisions: 0,
    items: [{ text: "Fix the export", status: "doing", done_when: "it works" }],
  };

  it("draws the two ways while they are tried", () => {
    act(() => root.render(<PlanCard plan={base} branching={0} />));
    const ways = [...container.querySelectorAll(".plan-ways li")].map((li) => li.textContent?.replace(/\s+/g, " ").trim());
    expect(ways).toEqual(["◧ Way A: the most direct fix", "◧ Way B: a different route"]);
  });

  it("says which was kept and why, and opens the other way on a click", async () => {
    branchView.mockResolvedValue({
      kept: "b",
      reason: "no new dependency",
      text: "Fix the export",
      ways: ["direct", "another way"],
      patches: ["+direct", "+other route"],
      files: [1, 1],
      children: [],
    });
    const kept: PlanView = {
      ...base,
      items: [{ ...base.items[0], branches: { kept: "b", reason: "no new dependency", ways: ["direct", "another way"], id: "abc" } }],
    };
    act(() => root.render(<PlanCard plan={kept} />));
    expect(container.querySelector(".plan-kept")?.textContent).toContain("Kept B: no new dependency.");
    const open = container.querySelector<HTMLButtonElement>(".plan-other")!;
    expect(open.textContent).toBe("See the other way");
    await act(async () => open.click());
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(branchView).toHaveBeenCalledWith("abc");
    // It opens on the way that was not kept, read-only.
    expect(dialog.querySelector(".branch-tab.on")?.textContent).toBe("Way A");
    expect(dialog.querySelector(".branch-patch")?.textContent).toBe("+direct");
    expect(dialog.querySelector(".branch-meta")?.textContent).toContain("not kept");
    const tabs = [...dialog.querySelectorAll<HTMLButtonElement>(".branch-tab")];
    expect(tabs[1].textContent).toBe("Way B · kept");
    await act(async () => tabs[1].click());
    expect(dialog.querySelector(".branch-patch")?.textContent).toBe("+other route");
    expect(dialog.querySelector(".branch-meta")?.textContent).toContain("kept: no new dependency");
  });

  it("says plainly when the record is gone", async () => {
    branchView.mockResolvedValue(null);
    const kept: PlanView = {
      ...base,
      items: [{ ...base.items[0], branches: { kept: "a", reason: "smaller", ways: ["x", "y"], id: "gone" } }],
    };
    act(() => root.render(<PlanCard plan={kept} />));
    await act(async () => container.querySelector<HTMLButtonElement>(".plan-other")!.click());
    expect(document.querySelector(".branch-empty")?.textContent).toBe("I no longer have this one.");
  });
});

describe("ⓘ on a move", () => {
  const plan: PlanView = {
    style: "lean",
    revisions: 0,
    items: [{ text: "Fix the export", status: "doing" }],
    moves: [
      {
        item: 0,
        move: "two-ways",
        rule: "two-ways-when-stuck",
        text: "When an item has failed twice, I try two ways and keep the better one.",
        from: "learned",
        since: "2026-10-30",
        state: "fired",
      },
      { move: "fresh-start", rule: "lean:head-full-fresh-start", text: "When my head is getting full, I start fresh with a handover note.", from: "preset", state: "fired" },
    ],
  };

  it("says the rule behind a move and where it comes from", () => {
    act(() => root.render(<PlanCard plan={plan} />));
    const card = rows()[0].querySelector(".plan-move-card")!;
    expect(card.querySelector(".plan-move-rule")?.textContent).toBe("When an item has failed twice, I try two ways and keep the better one.");
    expect(card.querySelector(".plan-move-from")?.textContent).toBe("Learned in October");
    // A move that was not about one item is told in the head.
    const head = container.querySelector(".plan-head .plan-move-card")!;
    expect(head.querySelector(".plan-move-from")?.textContent).toBe("Part of how I'm working");
  });

  it("offers Not this time only while the run is going, and sends it to the run", async () => {
    act(() => root.render(<PlanCard plan={plan} />));
    expect(container.querySelector(".plan-move-skip"), "a finished run has nothing to skip").toBeNull();

    act(() => root.render(<PlanCard plan={plan} runId="run1" />));
    const skip = rows()[0].querySelector<HTMLButtonElement>(".plan-move-skip")!;
    expect(skip.textContent).toBe("Not this time");
    await act(async () => skip.click());
    expect(skipMove).toHaveBeenCalledWith("run1", "two-ways-when-stuck");
    expect(rows()[0].querySelector(".plan-move-skip")).toBeNull();
    expect(rows()[0].textContent).toContain("I'll skip it once.");
  });

  it("says so when the run is over by the time it is clicked", async () => {
    skipMove.mockResolvedValue(false);
    act(() => root.render(<PlanCard plan={plan} runId="run1" />));
    await act(async () => rows()[0].querySelector<HTMLButtonElement>(".plan-move-skip")!.click());
    expect(rows()[0].textContent).toContain("That run is over.");
  });

  it("says a skipped move was skipped, and does not offer to skip it again", () => {
    const skipped: PlanView = { ...plan, moves: [{ ...plan.moves![0], state: "skipped" }] };
    act(() => root.render(<PlanCard plan={skipped} runId="run1" />));
    expect(rows()[0].querySelector(".plan-move-card")?.textContent).toContain("Skipped this time.");
    expect(rows()[0].querySelector(".plan-move-skip")).toBeNull();
  });
});

describe("a fresh start in the timeline", () => {
  const steps: AgentStep[] = [
    { id: "s1", verb: "read", target: "a.ts", status: "done" },
    { id: "item-0", verb: "", target: "", status: "done", divider: "Fresh start: I wrote myself a handover note", note: "I am building the export in src/export.rs." },
  ];

  it("is a line that opens the note, and the note is closed until asked for", () => {
    act(() => root.render(<Timeline steps={steps} live />));
    const line = container.querySelector<HTMLButtonElement>(".item-divider-btn")!;
    expect(line.textContent).toBe("Fresh start: I wrote myself a handover note");
    expect(container.querySelector(".item-divider-note")).toBeNull();
    act(() => line.click());
    expect(container.querySelector(".item-divider-note")?.textContent).toBe("I am building the export in src/export.rs.");
    act(() => line.click());
    expect(container.querySelector(".item-divider-note")).toBeNull();
  });

  it("is not counted among the steps", () => {
    act(() => root.render(<Timeline steps={steps} live />));
    expect(container.querySelector(".timeline-summary")?.textContent).toBe("1 step");
  });
});
