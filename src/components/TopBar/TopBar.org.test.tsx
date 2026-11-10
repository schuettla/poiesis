/**
 * @vitest-environment jsdom
 *
 * `WRK-15`: while a run is going the header mark opens how I am organised. When nothing
 * is running it is the mark and nothing more.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TopBar from "./TopBar";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

let container: HTMLDivElement;
let root: Root;
const openOrganization = vi.fn();

beforeEach(() => {
  openOrganization.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [] }],
    activeConversationId: "c1",
    view: "chat",
    itemTabs: [],
    activeItemId: null,
    railCollapsed: false,
    runSummaries: {},
    openOrganization,
  } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const live = (on: boolean) =>
  act(() =>
    useAppStore.setState({
      runSummaries: { c1: { runId: "r1", startedAt: 0, live: on, files: [], costUsd: null, localRun: false } },
    } as never)
  );

describe("the header mark during a run (WRK-15)", () => {
  it("is a button while a run is going, and opens the Coordination view", () => {
    act(() => root.render(<TopBar />));
    expect(container.querySelector(".brand-mark")).toBeNull();
    live(true);
    const button = container.querySelector<HTMLButtonElement>(".brand-mark")!;
    expect(button.getAttribute("aria-label")).toBe("Show how this chat is coordinated");
    expect(button.querySelector("svg.poiesis-mark")).not.toBeNull();
    act(() => button.click());
    expect(openOrganization).toHaveBeenCalled();
  });

  it("is only the mark again once the run is over", () => {
    act(() => root.render(<TopBar />));
    live(true);
    live(false);
    expect(container.querySelector(".brand-mark")).toBeNull();
    expect(container.querySelector(".brand svg.poiesis-mark")).not.toBeNull();
  });
});
