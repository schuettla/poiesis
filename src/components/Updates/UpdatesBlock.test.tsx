/**
 * @vitest-environment jsdom
 *
 * `UPD-UI-1`: every phase of the Updates block says what AUTOUPDATE_PLAN §5
 * says, and offers exactly the controls the spec lists for it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const restartApp = vi.fn(() => Promise.resolve());

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  setSetting: () => Promise.resolve(),
  restartApp: () => restartApp(),
}));

import { useAppStore } from "../../lib/store";
import type { UpdateState } from "../../lib/updates";
import UpdatesBlock from "./UpdatesBlock";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const info = { version: "0.1.2", notes: "Line one.\nLine two." };
let container: HTMLDivElement;
let root: Root;
const checkForUpdates = vi.fn(() => Promise.resolve());
const startUpdateInstall = vi.fn(() => Promise.resolve());

async function show(updateState: UpdateState) {
  useAppStore.setState({ updateState, checkForUpdates, startUpdateInstall });
  await act(async () => {
    root.render(<UpdatesBlock version="0.1.1" />);
  });
}

const text = () => container.textContent ?? "";
const buttons = () => Array.from(container.querySelectorAll("button")).map((b) => b.textContent);
const button = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent === label)!;

beforeEach(() => {
  restartApp.mockClear();
  checkForUpdates.mockClear();
  startUpdateInstall.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the Updates block", () => {
  it("promises never to install without asking", async () => {
    await show({ phase: "idle" });
    expect(text()).toContain("I'll never install one without asking you.");
    expect(text()).toContain("Check when I start");
  });

  it("idle: the version and a check button", async () => {
    await show({ phase: "idle" });
    expect(text()).toContain("Version 0.1.1");
    expect(buttons()).toEqual(["Check for updates"]);
    await act(async () => button("Check for updates").click());
    expect(checkForUpdates).toHaveBeenCalledWith(true);
  });

  it("checking: says so, with the button disabled", async () => {
    await show({ phase: "checking" });
    expect(text()).toContain("Checking…");
    expect(button("Check for updates").disabled).toBe(true);
  });

  it("current: up to date, and when it was checked", async () => {
    await show({ phase: "current", checkedAt: Date.now() });
    expect(text()).toContain("Version 0.1.1 — I'm up to date.");
    expect(text()).toContain("Checked just now");
    expect(buttons()).toEqual(["Check again"]);
  });

  it("available: the notes as written, and the two choices", async () => {
    await show({ phase: "available", info });
    expect(text()).toContain("Version 0.1.2 is available.");
    const notes = container.querySelector(".update-notes")!;
    expect(notes.textContent).toBe("Line one.\nLine two.");
    expect(buttons()).toEqual(["Download and install", "Not now"]);
    await act(async () => button("Download and install").click());
    expect(startUpdateInstall).toHaveBeenCalled();
  });

  it("available: Not now goes back to idle", async () => {
    await show({ phase: "available", info });
    await act(async () => button("Not now").click());
    expect(useAppStore.getState().updateState.phase).toBe("idle");
  });

  it("downloading: real byte counts, a bar, and no cancel", async () => {
    await show({ phase: "downloading", info, downloaded: 12_400_000, total: 48_100_000 });
    expect(text()).toContain("Downloading… 12.4 MB of 48.1 MB");
    const bar = container.querySelector<HTMLElement>(".dl-bar")!;
    expect(parseFloat(bar.style.width)).toBeCloseTo(25.8, 0);
    expect(buttons()).toEqual([]);
  });

  it("downloading with an unknown size: still honest, no fake percentage", async () => {
    await show({ phase: "downloading", info, downloaded: 2_000_000, total: null });
    expect(text()).toContain("Downloading… 2.0 MB");
    expect(text()).not.toContain("%");
  });

  it("installing: tells the user the app is about to restart", async () => {
    await show({ phase: "installing", info });
    expect(text()).toContain("I'll restart in a moment");
  });

  it("ready: restart now, or later — and later is a real answer", async () => {
    await show({ phase: "ready", info });
    expect(text()).toContain("Installed. I'll be version 0.1.2 once I restart.");
    expect(buttons()).toEqual(["Restart now", "Later"]);
    await act(async () => button("Later").click());
    expect(text()).toContain("takes effect the next time I start");
    expect(buttons()).toEqual([]);
  });

  it("ready: Restart now restarts", async () => {
    await show({ phase: "ready", info });
    await act(async () => button("Restart now").click());
    expect(restartApp).toHaveBeenCalled();
  });

  it("error: the mapped reason, never a raw string, and a retry", async () => {
    await show({ phase: "error", message: "that download didn't verify" });
    expect(text()).toContain("I couldn't check just now — that download didn't verify.");
    expect(buttons()).toEqual(["Try again"]);
  });

  it("the startup toggle is persisted through the store", async () => {
    await show({ phase: "idle" });
    const box = container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(box.checked).toBe(true);
    await act(async () => box.click());
    expect(useAppStore.getState().updateAutoCheck).toBe(false);
  });
});
