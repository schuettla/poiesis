/**
 * @vitest-environment jsdom
 *
 * `RTM-8`/`RTM-10`: Runtime's tabs, and the "Your servers" deep link the
 * picker and the Models page use. Rendered as the desktop app sees it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  // Never answers: the tabs are what's under test, not the status card.
  runtimeOverview: () => new Promise(() => {}),
  listEndpoints: () => Promise.resolve([]),
  listEndpointModels: () => Promise.resolve([]),
}));

import Runtime from "./Runtime";
import { useAppStore } from "../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const tabs = () =>
  Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]')).map((t) => ({
    label: t.textContent,
    selected: t.getAttribute("aria-selected") === "true",
  }));

describe("the Runtime page", () => {
  it("shows Chat, Images and Your servers to everyone, and Recall only in expert mode", async () => {
    useAppStore.setState({ expert: false, runtimeTab: null } as never);
    await act(async () => root.render(<Runtime />));
    expect(tabs().map((t) => t.label)).toEqual(["Chat", "Images", "Your servers"]);

    await act(async () => useAppStore.setState({ expert: true } as never));
    expect(tabs().map((t) => t.label)).toEqual(["Chat", "Images", "Your servers", "Recall"]);
  });

  it("opens Your servers from the deep link", async () => {
    useAppStore.setState({ expert: false, runtimeTab: "servers" } as never);
    await act(async () => root.render(<Runtime />));
    expect(tabs().find((t) => t.selected)?.label).toBe("Your servers");
    expect(container.textContent).toContain("Your own servers");
    // The link is spent once followed, so coming back later opens normally.
    expect(useAppStore.getState().runtimeTab).toBeNull();
  });
});
