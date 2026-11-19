/**
 * @vitest-environment jsdom
 *
 * `TEAM-23`: a change Poiesis would like to make to a team badges Personas & teams, where it is answered
 * and where it writes, and not Self. A change to how I work badges Self as before.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import SettingsHub from "./SettingsHub";
import { useAppStore } from "../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

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

const proposal = (target: string) => ({ id: `p-${target}`, target, status: "pending", proposed_text: "{}", rationale: "" });

function badged(label: string): boolean {
  const tab = [...container.querySelectorAll(".settings-hub-tab")].find((t) => t.querySelector(".sht-label")?.textContent === label);
  return !!tab?.querySelector(".sht-badge");
}

function show(targets: string[]) {
  act(() => {
    useAppStore.setState({ view: "settings", changeProposals: targets.map(proposal) } as never);
    root.render(<SettingsHub />);
  });
}

describe("what is waiting in Settings", () => {
  it("a change to a team badges Personas & teams and leaves Self quiet", () => {
    show(["team"]);
    expect(badged("Personas & teams")).toBe(true);
    expect(badged("Self")).toBe(false);
  });

  it("a change to how I work badges Self and not Personas & teams", () => {
    show(["way"]);
    expect(badged("Self")).toBe(true);
    expect(badged("Personas & teams")).toBe(false);
  });

  it("nothing waiting, no badge", () => {
    show([]);
    expect(badged("Self")).toBe(false);
    expect(badged("Personas & teams")).toBe(false);
  });
});
