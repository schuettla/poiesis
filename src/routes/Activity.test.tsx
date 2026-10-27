/**
 * @vitest-environment jsdom
 *
 * `CTL-UI-1`: decisions in the Activity list.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActivityEntry, DecisionEntry } from "../lib/api";

const listActivity = vi.fn<() => Promise<ActivityEntry[]>>();
const decisionsList = vi.fn<() => Promise<DecisionEntry[]>>();
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  listActivity: () => listActivity(),
  decisionsList: () => decisionsList(),
}));

import Activity from "./Activity";
import { useAppStore } from "../lib/store";
import { decision } from "../lib/decisions.fixture";
import { experiment } from "../lib/experiments.fixture";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const text = () => container.textContent ?? "";
const flush = () => act(async () => {});
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const line = (needle: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>(".activity-decision-line")).find((b) =>
    b.textContent?.includes(needle)
  );

const FILE: ActivityEntry = { id: "a1", conversation_id: null, kind: "file", detail: "read notes.txt", created_at: 1000 };

async function mount() {
  await act(async () => root.render(<Activity />));
  await flush();
}

beforeEach(() => {
  listActivity.mockReset().mockResolvedValue([FILE]);
  decisionsList.mockReset().mockResolvedValue([]);
  useAppStore.setState({ experiments: null, activityFilter: null });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("decisions in Activity (CTL-UI-1)", () => {
  it("shows no filter chips and no decision row when there are no decisions", async () => {
    await mount();
    expect(text()).toContain("read notes.txt");
    expect(container.querySelector(".activity-filters")).toBeNull();
    expect(container.querySelector(".activity-decision")).toBeNull();
  });

  it("shows a decision as one collapsed line, with the numbers", async () => {
    decisionsList.mockResolvedValue([decision({ at: 5000 })]);
    await mount();
    expect(line("Decided")?.textContent).toContain("Is the goal reached? · yes 0.93, no 0.07 · 41 ms · this computer");
    expect(line("Decided")?.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelector(".decision-detail")).toBeNull();
  });

  it("expanding shows what was asked, a bar per option, what was done, and how it turned out", async () => {
    decisionsList.mockResolvedValue([
      decision({
        at: 5000,
        labels: [{ label: "weak_wrong", value: null, source: "goal-replaced" }],
      }),
    ]);
    await mount();
    await click(line("Decided"));
    expect(line("Decided")?.getAttribute("aria-expanded")).toBe("true");
    const detail = container.querySelector(".decision-detail")!;
    expect(detail.textContent).toContain("Goal: make the tests pass");
    expect(detail.querySelectorAll(".decision-bar")).toHaveLength(2);
    expect(detail.querySelector<HTMLElement>(".decision-fill")!.style.width).toBe("93%");
    expect(detail.textContent).toContain("Acted on it");
    expect(detail.textContent).toContain("Not calibrated yet");
    expect(detail.textContent).toContain("Afterwards: Probably wrong");
  });

  it("a decision that a limit stopped says so, with no bars", async () => {
    decisionsList.mockResolvedValue([decision({ at: 5000, ms: 0, answers: {}, action: "budget:repeat" })]);
    await mount();
    await click(line("Decided"));
    expect(container.querySelector(".decision-bar")).toBeNull();
    expect(container.querySelector(".decision-action")?.textContent).toContain("already asked");
  });

  it("folds background decisions into one line an hour, and expanding lists them", async () => {
    const at = 10 * 3_600_000;
    const bg = (id: string, offset: number) =>
      decision({ id, at: at + offset, purpose: "mem-scope", label: "Does a saved note apply always?", background: true });
    decisionsList.mockResolvedValue([bg("1", 1), bg("2", 2), bg("3", 3)]);
    await mount();
    expect(container.querySelectorAll(".activity-decision")).toHaveLength(1);
    expect(line("3 decisions")?.textContent).toContain("Does a saved note apply always?");
    await click(line("3 decisions"));
    expect(container.querySelectorAll(".decision-detail")).toHaveLength(3);
  });

  it("filters to actions or decisions, and to an experiment's own", async () => {
    useAppStore.setState({ experiments: [experiment({ id: "fast-slow", title: "Fast and slow thinking" })] });
    decisionsList.mockResolvedValue([
      decision({ id: "d1", at: 5000 }),
      decision({ id: "d2", at: 6000, experiment: "fast-slow", purpose: "run-if", label: "Should it run?" }),
    ]);
    await mount();
    const chip = (name: string) =>
      Array.from(container.querySelectorAll<HTMLButtonElement>(".activity-chip")).find((b) => b.textContent === name);
    expect(Array.from(container.querySelectorAll(".activity-chip")).map((b) => b.textContent)).toEqual([
      "Everything",
      "Actions",
      "Decisions",
      "Fast and slow thinking",
    ]);

    await click(chip("Actions"));
    expect(container.querySelectorAll(".activity-decision")).toHaveLength(0);
    expect(text()).toContain("read notes.txt");

    await click(chip("Decisions"));
    expect(container.querySelectorAll(".activity-decision")).toHaveLength(2);
    expect(text()).not.toContain("read notes.txt");

    await click(chip("Fast and slow thinking"));
    expect(container.querySelectorAll(".activity-decision")).toHaveLength(1);
    expect(text()).toContain("Should it run?");
  });

  it("opens already narrowed when an experiment's card sent it here, and only that once", async () => {
    useAppStore.setState({
      activityFilter: "experiment:fast-slow",
      experiments: [experiment({ id: "fast-slow", title: "Fast and slow thinking" })],
    });
    decisionsList.mockResolvedValue([
      decision({ id: "d1", at: 5000 }),
      decision({ id: "d2", at: 6000, experiment: "fast-slow", purpose: "tool-select", label: "Does this message need a group of tools?" }),
    ]);
    await mount();
    expect(container.querySelectorAll(".activity-decision")).toHaveLength(1);
    expect(text()).toContain("Does this message need a group of tools?");
    expect(text()).not.toContain("read notes.txt");
    expect(useAppStore.getState().activityFilter).toBeNull();
  });
});
