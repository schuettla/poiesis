/**
 * @vitest-environment jsdom
 *
 * `SLP-1`: the app tells the backend when it is in use, so the daily tidy-up can wait
 * for a quiet app and stop on a key.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls: boolean[] = [];
vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inTauri: () => true,
  noteUserInput: (stop: boolean) => {
    calls.push(stop);
    return Promise.resolve();
  },
}));

import { useAppStore } from "./store";
import { BUSY_BEAT_MS, INPUT_THROTTLE_MS, useReportUse } from "./useReportUse";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function Probe() {
  useReportUse();
  return null;
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  calls.length = 0;
  useAppStore.setState({ busy: false } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

describe("reporting use", () => {
  it("reports a key press, throttled, as input that stops a pass", () => {
    act(() => root.render(<Probe />));
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" })));
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "b" })));
    expect(calls).toEqual([true]);
    act(() => void vi.advanceTimersByTime(INPUT_THROTTLE_MS + 1));
    act(() => void window.dispatchEvent(new Event("pointerdown")));
    expect(calls).toEqual([true, true]);
  });

  it("says a turn is streaming, now and on a beat, without ending anything", () => {
    act(() => root.render(<Probe />));
    act(() => useAppStore.setState({ busy: true } as never));
    expect(calls).toEqual([false]);
    act(() => void vi.advanceTimersByTime(BUSY_BEAT_MS));
    expect(calls).toEqual([false, false]);
    act(() => useAppStore.setState({ busy: false } as never));
    act(() => void vi.advanceTimersByTime(BUSY_BEAT_MS * 2));
    expect(calls).toEqual([false, false]);
  });
});
