/**
 * @vitest-environment jsdom
 *
 * Every tab in the settings navigation actually routes to something.
 *
 * The bug this pins: `App` decided whether to render the hub from its own
 * hand-written copy of the tab list. "usage" was added to the hub's `TABS` and
 * never to that copy, so choosing Usage rendered *nothing at all* — no hub, no
 * side navigation, no panel, just an empty window. It looked like the Usage
 * panel was broken; the panel was never mounted.
 *
 * A duplicated list is the defect, so the test is about the two agreeing rather
 * than about Usage specifically.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import SettingsHub, { isHubView } from "./SettingsHub";
import { useAppStore } from "../lib/store";
import type { View } from "../lib/types";

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

function renderAt(view: View) {
  act(() => {
    useAppStore.setState({ view } as never);
    root.render(<SettingsHub />);
  });
}

/** The tabs as the hub itself renders them — read off the DOM rather than
 * imported, so this cannot pass by sharing the same mistake. */
function renderedTabs(): string[] {
  return Array.from(container.querySelectorAll(".settings-hub-tab .sht-label")).map(
    (el) => el.textContent ?? ""
  );
}

describe("the settings hub", () => {
  it("routes every tab it draws", () => {
    renderAt("settings");
    const labels = renderedTabs();
    expect(labels.length).toBeGreaterThan(5);
    expect(labels).toContain("Usage");
  });

  it("claims every view its own navigation can reach", () => {
    // `isHubView` is what `App` asks before mounting the hub at all. If a tab
    // exists that it does not claim, choosing that tab blanks the window.
    renderAt("settings");
    const byLabel: Record<string, View> = {
      General: "settings",
      Models: "models",
      Providers: "providers",
      Runtime: "runtime",
      Tools: "tools",
      Skills: "skills",
      Apps: "apps",
      Self: "self",
      Tasks: "tasks",
      Mail: "mail",
      Activity: "activity",
      Usage: "usage",
      "Working dir": "workingdir",
      About: "about",
    };
    for (const label of renderedTabs()) {
      const view = byLabel[label];
      expect(view, `no view mapped for the tab "${label}"`).toBeDefined();
      expect(isHubView(view), `"${label}" draws a tab but App would render nothing`).toBe(true);
    }
  });

  it("does not claim the views that are not the hub's", () => {
    // Chat and Library render themselves; if the hub claimed them, two panels
    // would mount over one another.
    expect(isHubView("chat")).toBe(false);
    expect(isHubView("library")).toBe(false);
  });

  it("mounts the Usage panel when Usage is the view", () => {
    renderAt("usage");
    expect(container.querySelector(".usage-panel"), "the panel itself is missing").not.toBeNull();
    expect(container.querySelector(".settings-hub-nav"), "the nav went with it").not.toBeNull();
  });

  it("routes Providers and Runtime, and has no Engine any more", () => {
    act(() => useAppStore.setState({ expert: false } as never));
    renderAt("providers");
    expect(container.querySelector("h1")?.textContent).toBe("Providers");
    const labels = renderedTabs();
    // `RTM-9`: Runtime is a normal nav item for everyone, after Providers.
    expect(labels.indexOf("Runtime")).toBe(labels.indexOf("Providers") + 1);
    expect(labels.indexOf("Providers")).toBe(labels.indexOf("Models") + 1);
    expect(labels).not.toContain("Engine");
    renderAt("runtime");
    expect(container.querySelector("h1")?.textContent).toBe("Runtime");
  });

  it("General points to Providers and Runtime instead of holding keys and servers", () => {
    renderAt("settings");
    const text = container.textContent ?? "";
    expect(text).not.toContain("Cloud models — your keys");
    expect(text).not.toContain("Your own model servers");
  });

  it("keeps its own navigation whatever the rail is doing", () => {
    // `SHL-16` is withdrawn. These sections are the *inside of Settings*, so
    // they navigate from inside Settings — not from the Rail, which lists the
    // conversations and projects and keeps doing so while you are in here.
    for (const railCollapsed of [true, false]) {
      act(() => {
        useAppStore.setState({ view: "usage", railCollapsed } as never);
        root.render(<SettingsHub />);
      });
      expect(container.querySelector(".settings-hub-nav"), "the hub navigates itself").not.toBeNull();
      expect(container.querySelector(".usage-panel")).not.toBeNull();
    }
  });
});
