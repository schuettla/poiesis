/**
 * @vitest-environment jsdom
 *
 * `REM-UI-2`: the mark wears one fine ring while remote is on and a phone is connected, and says
 * so in its label. Nothing else changes.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import PoiesisMark from "./PoiesisMark";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => useAppStore.setState({ presence: "idle", remoteReachable: false, runSummaries: {}, subRuns: {} }));
  act(() => root.render(<PoiesisMark />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  act(() => useAppStore.setState({ remoteReachable: false }));
});

const mark = () => container.querySelector("svg.poiesis-mark")!;

describe("the mark when a phone can reach me", () => {
  it("has no ring while nobody can", () => {
    expect(mark().querySelector(".mark-reach")).toBeNull();
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — resting");
  });

  it("draws one ring and says so while a phone is connected", () => {
    act(() => useAppStore.setState({ remoteReachable: true }));
    expect(mark().querySelectorAll(".mark-reach")).toHaveLength(1);
    expect(mark().classList.contains("reachable")).toBe(true);
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — resting · your phone can reach me");
  });

  it("keeps the ring while it works, and drops it when the phone goes", () => {
    act(() => useAppStore.setState({ remoteReachable: true, presence: "active" }));
    expect(mark().getAttribute("aria-label")).toBe("Poiesis Agent — working · your phone can reach me");
    act(() => useAppStore.setState({ remoteReachable: false }));
    expect(mark().querySelector(".mark-reach")).toBeNull();
  });
});
