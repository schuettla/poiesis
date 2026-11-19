/**
 * @vitest-environment jsdom
 *
 * `TEAM-21`: the switch "Tell me when someone is waiting for me" is on until turned off, and
 * writes the key the backend reads.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({ value: null as string | null, writes: [] as [string, string][] }));

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  getSetting: async () => store.value,
  setSetting: async (k: string, v: string) => {
    store.writes.push([k, v]);
    store.value = v;
  },
}));

import NotifyWaiting from "./NotifyWaiting";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  store.value = null;
  store.writes = [];
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const box = () => container.querySelector<HTMLInputElement>("input[type=checkbox]")!;

describe("the notification switch", () => {
  it("is on when nothing was ever set", async () => {
    await act(async () => root.render(<NotifyWaiting />));
    expect(box().checked).toBe(true);
    expect(container.textContent).toContain("Tell me when someone is waiting for me");
  });

  it("reads an off that was saved, and writes the key the backend reads", async () => {
    store.value = "off";
    await act(async () => root.render(<NotifyWaiting />));
    expect(box().checked).toBe(false);
    await act(async () => box().click());
    expect(store.writes).toEqual([["notify.waiting", "on"]]);
    await act(async () => box().click());
    expect(store.writes[1]).toEqual(["notify.waiting", "off"]);
  });
});
