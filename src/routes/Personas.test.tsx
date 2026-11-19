/**
 * @vitest-environment jsdom
 *
 * `TEAM-4`: the Greek line under the intro is muted and can be folded away, and comes back
 * with the other first-time explanations. `TEAM-9`: Personas and Teams are two tabs; a link
 * that names a team lands on Teams.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../components/Personas/PersonaEditor", () => ({ default: () => <div id="personas-tab" /> }));
vi.mock("../components/Personas/TeamEditor", () => ({ default: () => <div id="teams-tab" /> }));

import Personas from "./Personas";
import { useAppStore } from "../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ firstTimeFlags: {}, personasFocus: null });
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const line = () => container.textContent?.includes("koryphaios");

describe("the Greek line", () => {
  it("is there until it is folded away, and the first line always is", () => {
    act(() => root.render(<Personas />));
    expect(line()).toBe(true);
    act(() => Array.from(container.querySelectorAll("button")).find((b) => b.textContent === "Hide")!.click());
    expect(line()).toBe(false);
    expect(container.textContent).toContain("A persona is someone Poiesis speaks as");
    expect(useAppStore.getState().firstTimeFlags.koryphaios).toBe(true);
  });

  it("comes back with the first-time explanations", async () => {
    useAppStore.setState({ firstTimeFlags: { koryphaios: true } });
    act(() => root.render(<Personas />));
    expect(line()).toBe(false);
    await act(async () => useAppStore.getState().resetFirstTimeExplanations());
    expect(line()).toBe(true);
  });
});

describe("the tabs", () => {
  const shown = (id: string) => !container.querySelector(`#${id}`)!.closest("[role=tabpanel]")!.hasAttribute("hidden");

  it("open on Personas, and both stay mounted so nothing typed on the other is lost", () => {
    act(() => root.render(<Personas />));
    const tabs = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
    expect(tabs.map((t) => t.textContent)).toEqual(["Personas", "Teams"]);
    expect([shown("personas-tab"), shown("teams-tab")]).toEqual([true, false]);
    act(() => tabs[1].click());
    expect([shown("personas-tab"), shown("teams-tab")]).toEqual([false, true]);
    expect(tabs[1].getAttribute("aria-selected")).toBe("true");
  });

  it("a link to a team lands on Teams, one to a persona on Personas", () => {
    useAppStore.setState({ personasFocus: "team:shop" });
    act(() => root.render(<Personas />));
    expect([shown("personas-tab"), shown("teams-tab")]).toEqual([false, true]);
    act(() => useAppStore.setState({ personasFocus: "p1" }));
    expect([shown("personas-tab"), shown("teams-tab")]).toEqual([true, false]);
  });
});
