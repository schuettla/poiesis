/**
 * @vitest-environment jsdom
 *
 * The app renders at all.
 *
 * This exists because it didn't, and nothing caught it: a zustand v5 selector
 * that built a fresh array (`s.skills.filter(...)`) made React's
 * `useSyncExternalStore` see a changed snapshot on every pass, which loops
 * until React throws and unmounts the tree — a blank window, with a clean
 * Rust log and a clean `tsc --noEmit`. Type-checking cannot see it and the
 * `lib/` unit tests never mount a component, so only a render catches it.
 *
 * Deliberately shallow: mounting the real `App` against the real store, with
 * no Tauri runtime (every `inTauri()` path no-ops), asserting only that the
 * tree commits and stays committed. Anything more specific would be a test of
 * the layout rather than of the thing that broke.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { useAppStore } from "./lib/store";

// React 18 reads this to decide whether `act` warnings apply.
(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom implements no scrolling at all, and Chat's stick-to-bottom effect
// calls `scrollTo` on mount. Stubbing the missing method is not papering over
// an app bug — there is no layout in jsdom for it to act on.
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
  vi.restoreAllMocks();
});

/** Mount `App` and fail loudly on the render loop, which surfaces as a thrown
 * "Maximum update depth exceeded" rather than as a rejected promise. */
function mount() {
  act(() => {
    root.render(<App />);
  });
}

describe("App renders", () => {
  it("mounts without throwing and puts something in the DOM", () => {
    mount();
    expect(container.querySelector(".app")).not.toBeNull();
    expect(container.textContent?.trim().length ?? 0).toBeGreaterThan(0);
  });

  it("survives a skills list arriving, the shape that caused the blank window", () => {
    mount();
    // The store starts with `skills: []`, so an empty list alone would not
    // have caught the original bug — the loop needs a non-empty array the
    // selector rebuilds each pass.
    act(() => {
      useAppStore.setState({
        skills: [
          {
            name: "weekly-report",
            description: "Draft the weekly report.",
            when_to_use: null,
            source: "app",
            dir: "/skills/weekly-report",
            enabled: true,
            unsupported: [],
            used: 0,
            rough: 0,
            risk: 0,
            risk_flags: [],
          },
          {
            name: "off-skill",
            description: "Not enabled.",
            when_to_use: null,
            source: "personal",
            dir: "/skills/off-skill",
            enabled: false,
            unsupported: [],
            used: 0,
            rough: 0,
            risk: 0,
            risk_flags: [],
          },
        ],
      });
    });
    expect(container.querySelector(".app")).not.toBeNull();
  });

  it("renders every settings-hub tab, including Skills", () => {
    // `View` gained "skills" but `App`'s hub condition did not, so selecting
    // the tab rendered an empty shell. Each view must commit something.
    for (const view of ["settings", "models", "engine", "apps", "skills", "self", "tasks"] as const) {
      act(() => {
        useAppStore.setState({ view });
      });
      mount();
      expect(container.querySelector(".settings-hub"), `${view} renders the hub`).not.toBeNull();
    }
  });
});
