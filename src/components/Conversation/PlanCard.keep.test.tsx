/**
 * @vitest-environment jsdom
 *
 * `WRK-17`: a run that is over can be kept as a preset. The dialog opens with the one
 * the run worked under and the moves that fired that did not come from it; a live run
 * has no such menu, and neither has a run in no preset.
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
const openPresetDialog = vi.fn();

beforeEach(() => {
  openPresetDialog.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ presets: [], openPresetDialog } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const move = (over: Record<string, unknown>) => ({
  move: "advice",
  when: "risky-action",
  rule: "r",
  text: "t",
  from: "learned",
  state: "fired",
  ...over,
});

const plan = (over: Partial<PlanView> = {}): PlanView =>
  ({
    revisions: 0,
    style: "lean",
    items: [{ text: "One", status: "done" }],
    moves: [
      move({ rule: "lean:start-one-at-a-time", move: "one-at-a-time", when: "start", from: "preset" }),
      move({ rule: "way-1", move: "advice", when: "risky-action", from: "learned" }),
      move({ rule: "way-1", move: "advice", when: "risky-action", from: "learned", item: 0 }),
      move({ rule: "way-2", move: "second-look", when: "before-done", from: "trying", state: "skipped" }),
      move({ rule: "stop", move: "stop-the-line", when: "check-fails", from: "always" }),
    ],
    ...over,
  }) as PlanView;

const more = () => container.querySelector<HTMLButtonElement>(".plan-more-btn");

describe("keeping a way of working as a preset", () => {
  it("opens the dialog with the preset it ran under and the learned moves that fired, once each", () => {
    act(() => root.render(<PlanCard plan={plan()} />));
    act(() => more()!.click());
    act(() => (container.querySelector(".row-menu-item") as HTMLButtonElement).click());
    expect(openPresetDialog).toHaveBeenCalledWith({
      mode: "new",
      from: "lean",
      add: [{ when: "risky-action", move: "advice" }],
    });
  });

  it("is not offered while the run is going, or for a run in no preset", () => {
    act(() => root.render(<PlanCard plan={plan()} runId="r1" />));
    expect(more()).toBeNull();
    act(() => root.render(<PlanCard plan={plan({ style: undefined })} />));
    expect(more()).toBeNull();
  });
});
