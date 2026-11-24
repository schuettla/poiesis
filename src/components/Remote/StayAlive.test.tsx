/**
 * @vitest-environment jsdom
 *
 * `REM-UI-2`: "Keep running when I close the window" is on until turned off and writes the key the
 * backend reads; "Start when I sign in" follows the real autostart state and says so when it fails.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({
  keep: null as string | null,
  start: false,
  writes: [] as [string, string][],
  failStart: false,
}));

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  getSetting: async () => store.keep,
  setSetting: async (k: string, v: string) => {
    store.writes.push([k, v]);
    store.keep = v;
  },
  autostartStatus: async () => store.start,
  autostartSet: async (on: boolean) => {
    if (store.failStart) throw new Error("Not allowed");
    store.start = on;
    return on;
  },
}));

import StayAlive from "./StayAlive";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(store, { keep: null, start: false, writes: [], failStart: false });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const boxes = () => Array.from(container.querySelectorAll<HTMLInputElement>("input[type=checkbox]"));

describe("the stay-alive switches", () => {
  it("keeps running by default and starts at sign-in only when asked", async () => {
    await act(async () => root.render(<StayAlive />));
    const [keep, start] = boxes();
    expect(keep.checked).toBe(true);
    expect(start.checked).toBe(false);
    expect(container.textContent).toContain("Keep running when I close the window");
    expect(container.textContent).toContain("Start when I sign in");
  });

  it("writes the key the backend reads", async () => {
    store.keep = "off";
    await act(async () => root.render(<StayAlive />));
    expect(boxes()[0].checked).toBe(false);
    await act(async () => boxes()[0].click());
    expect(store.writes).toEqual([["app.keep_running", "on"]]);
    await act(async () => boxes()[0].click());
    expect(store.writes[1]).toEqual(["app.keep_running", "off"]);
  });

  it("turns the login start on, and puts the box back with the reason when it cannot", async () => {
    await act(async () => root.render(<StayAlive />));
    await act(async () => boxes()[1].click());
    expect(store.start).toBe(true);
    expect(boxes()[1].checked).toBe(true);
    store.failStart = true;
    await act(async () => boxes()[1].click());
    expect(boxes()[1].checked).toBe(true);
    expect(container.textContent).toContain("Not allowed");
  });
});
