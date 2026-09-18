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
    for (const view of ["settings", "models", "runtime", "apps", "skills", "self", "tasks"] as const) {
      act(() => {
        useAppStore.setState({ view });
      });
      mount();
      expect(container.querySelector(".settings-hub"), `${view} renders the hub`).not.toBeNull();
    }
  });
});

/**
 * `SHL-18-T`/`SHL-24`: what is left of the strip's keyboard bindings, and the
 * rule that matters most about them — every one is skipped while a text field
 * has focus. A shortcut that closes the thing you are halfway through typing
 * into is worse than no shortcut at all.
 *
 * Only `Ctrl+W` remains, and it only ever closes an item tab. `Ctrl+Tab` and
 * `Ctrl+1..9` addressed a list of open chats; chats are destinations now, so
 * there is no such list and nothing for those to address.
 */
describe("the strip's keyboard bindings (SHL-18)", () => {
  const press = (key: string, opts: KeyboardEventInit = {}) =>
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey: true, bubbles: true, ...opts }));
    });

  const seedTabs = () =>
    act(() => {
      useAppStore.setState({
        view: "chat",
        conversations: [
          // A folder, so that a file tab inside it is a real tab: without one
          // the item view correctly closes it as a ghost, and a test opening
          // it would be measuring that cleanup instead of the binding.
          { id: "c1", title: "c1", updatedAt: 0, messages: [], folderPath: "C:\\work" },
          { id: "c2", title: "c2", updatedAt: 0, messages: [] },
        ],
        activeConversationId: "c1",
        itemTabs: [],
        activeItemId: null,
      });
    });

  it("Ctrl+W closes the active item tab", () => {
    mount();
    seedTabs();
    act(() => {
      useAppStore.getState().openItem({ kind: "file", id: "C:\\work\\notes.md" });
    });
    press("w");
    const s = useAppStore.getState();
    expect(s.itemTabs).toEqual([]);
    expect(s.activeItemId).toBeNull();
    expect(s.activeConversationId, "the chat behind it is not touched").toBe("c1");
  });

  it("does nothing with no item open — a chat is not a thing you close", () => {
    mount();
    seedTabs();
    press("w");
    const s = useAppStore.getState();
    expect(s.activeConversationId).toBe("c1");
    expect(s.conversations).toHaveLength(2);
  });

  it("hands the whole main column to an open item, without unmounting the chat", () => {
    // `SHL-27`: the conversation steps aside rather than being torn down —
    // its scroll position, the composer's draft and any running turn have to
    // survive a look at a file. The shell says so with one class; the chat's
    // own markup stays in the tree behind it.
    mount();
    seedTabs();
    act(() => {
      useAppStore.getState().openItem({ kind: "file", id: "C:\\work\\notes.md" });
    });
    expect(container.querySelector(".app")?.className).toContain("item-open");
    expect(container.querySelector(".item-view"), "the item is showing").not.toBeNull();
    expect(container.querySelector(".chat-body"), "the chat is still mounted").not.toBeNull();

    // And the session tab puts it back without closing anything.
    act(() => useAppStore.getState().showConversation());
    expect(container.querySelector(".app")?.className).not.toContain("item-open");
    expect(container.querySelector(".item-view")).toBeNull();
    expect(useAppStore.getState().itemTabs, "the tab is still open").toHaveLength(1);
  });

  it("is skipped while a text field has focus", () => {
    mount();
    seedTabs();
    act(() => {
      useAppStore.getState().openItem({ kind: "file", id: "C:\\work\\notes.md" });
    });
    const input = document.createElement("textarea");
    document.body.appendChild(input);
    input.focus();

    press("w");

    expect(useAppStore.getState().itemTabs, "typing must not close the tab").toHaveLength(1);
    input.remove();
  });
});
