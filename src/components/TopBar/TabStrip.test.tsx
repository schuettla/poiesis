/**
 * @vitest-environment jsdom
 *
 * The header strip's behaviour in the browser: the parts of `SHL-18`,
 * `SHL-19` and `SHL-24` that only exist once something is actually rendered
 * and can be clicked, typed at, or focused.
 *
 * Runs with `api.inTauri()` false, so persistence is skipped and only the
 * local state transitions are under test.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import TabStrip from "./TabStrip";
import { useAppStore } from "../../lib/store";
import type { Conversation } from "../../lib/types";
import type { ChangeSet, FileChange } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};
Element.prototype.scrollIntoView = () => {};

const FOLDER = "C:\\work";

function conv(id: string): Conversation {
  return { id, title: id, updatedAt: Date.now(), messages: [], folderPath: FOLDER };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    conversations: [conv("c1"), conv("c2")],
    activeConversationId: "c1",
    itemTabs: [],
    activeItemId: null,
    selected: null,
    view: "chat",
    artifacts: {},
    dockOpen: true,
    railCollapsed: false,
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const renderStrip = () =>
  act(() => {
    root.render(<TabStrip />);
  });

/** Every tab in the strip, the session tab included — the order the keyboard
 * and the chevrons walk. */
const tabs = () => Array.from(container.querySelectorAll<HTMLElement>(".ts-tab"));
/** Just the items, for the assertions that are about what was opened rather
 * than about the strip as a whole. */
const itemTabs = () => Array.from(container.querySelectorAll<HTMLElement>(".ts-tab:not(.ts-session)"));
const labelOf = (t: Element | null | undefined) => t?.querySelector(".ts-tab-label")?.textContent ?? "";
const labels = () => itemTabs().map(labelOf);

const files = (...names: string[]) =>
  names.map((n) => ({ kind: "file" as const, id: `${FOLDER}\\${n}`, conversationId: "c1" }));

describe("the tab strip (SHL-18/SHL-19/SHL-24/SHL-27)", () => {
  it("holds this chat and the items opened out of it, and nothing else", () => {
    // `SHL-24`: other chats, projects and Settings are destinations, not tabs.
    // The strip is one tablist: this conversation, then what was picked out of
    // the right sidebar while you were in it.
    act(() => {
      useAppStore.setState({ itemTabs: files("notes.md", "main.rs"), activeItemId: `file:${FOLDER}\\main.rs` });
    });
    renderStrip();

    expect(container.querySelectorAll('[role="tablist"]')).toHaveLength(1);
    expect(tabs().map(labelOf)).toEqual(["c1", "notes.md", "main.rs"]);
    // The live chat is the only chat named here; no route tab, and no way to
    // start a chat from the strip.
    expect(container.querySelectorAll(".ts-session")).toHaveLength(1);
    expect(container.querySelector(".ts-route")).toBeNull();
    expect(container.querySelector(".ts-new"), "no new-chat button in the strip").toBeNull();
  });

  it("draws nothing at all with no item open", () => {
    // `SHL-27`: the session tab exists *because* something else is open. Alone
    // it would be a control pointing at the page already in front of you.
    renderStrip();
    expect(container.querySelector('[role="tablist"]')).toBeNull();
    expect(container.querySelector(".ts-session")).toBeNull();
  });

  it("puts the conversation first, and it cannot be closed", () => {
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md"), activeItemId: `file:${FOLDER}\\a.md` });
    });
    renderStrip();
    const session = tabs()[0];
    expect(session.className).toContain("ts-session");
    expect(session.querySelector(".ts-tab-close"), "no × on a destination").toBeNull();
    // Nor by the gesture that closes every other tab.
    act(() => {
      session.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 1 }));
    });
    expect(useAppStore.getState().itemTabs).toHaveLength(1);
  });

  it("shows the conversation again without closing what is open", () => {
    // What separates this tab from closing the item: the tabs stay, so coming
    // back to one costs a click rather than finding it in the sidebar again.
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md"), activeItemId: `file:${FOLDER}\\b.md` });
    });
    renderStrip();
    act(() => tabs()[0].click());
    const s = useAppStore.getState();
    expect(s.activeItemId).toBeNull();
    expect(s.itemTabs).toHaveLength(2);
    expect(tabs()[0].getAttribute("aria-selected")).toBe("true");
  });

  it("draws nothing on a route, where there is no sidebar to stand over", () => {
    act(() => {
      useAppStore.setState({ itemTabs: files("notes.md"), view: "settings" });
    });
    renderStrip();
    expect(container.querySelector('[role="tablist"]')).toBeNull();
  });

  it("falls back to the conversation when nothing names an open item", () => {
    // A null `activeItemId` is a real state — the session tab is selected —
    // and so is one naming an item that is gone. Both land on the chat, the
    // one tab in the strip that always exists.
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md"), activeItemId: `file:${FOLDER}\\vanished.md` });
    });
    renderStrip();
    const active = container.querySelectorAll(".ts-tab.active");
    expect(active).toHaveLength(1);
    expect(active[0].className).toContain("ts-session");
  });

  it("shows only the live chat's items, so no tab can switch the conversation", () => {
    act(() => {
      useAppStore.setState({
        itemTabs: [...files("mine.md"), { kind: "file", id: `${FOLDER}\\theirs.md`, conversationId: "c2" }],
      });
    });
    renderStrip();
    expect(labels()).toEqual(["mine.md"]);
  });

  it("closes a tab on middle-click of the tab itself, not only its close button", () => {
    // The gesture is worth nothing on the close button: a plain click already
    // closes that. It has to work where the pointer actually is — the tab.
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md", "c.md") });
    });
    renderStrip();
    act(() => {
      itemTabs()[1].dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 1 }));
    });
    expect(labels()).toEqual(["a.md", "c.md"]);
  });

  it("leaves the tab set alone on a left-click mousedown", () => {
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md", "c.md") });
    });
    renderStrip();
    act(() => {
      itemTabs()[1].dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    });
    expect(labels()).toEqual(["a.md", "b.md", "c.md"]);
  });

  it("is one stop in the page's tab order, not one per tab", () => {
    // `SHL-19`: roving focus. Only the active tab is reachable with Tab; the
    // rest are reached with the arrow keys, from inside.
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md"), activeItemId: `file:${FOLDER}\\a.md` });
    });
    renderStrip();
    const reachable = tabs().filter((t) => t.tabIndex === 0);
    expect(reachable).toHaveLength(1);
    expect(labelOf(reachable[0])).toBe("a.md");
  });

  it("moves focus along the strip with the arrow keys and to its ends with Home/End", () => {
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md", "c.md") });
    });
    renderStrip();
    const zone = container.querySelector<HTMLElement>(".ts-zone")!;
    tabs()[0].focus();

    const press = (key: string) =>
      act(() => {
        zone.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      });

    press("ArrowRight");
    expect(document.activeElement).toBe(tabs()[1]);
    press("End");
    expect(document.activeElement).toBe(tabs()[tabs().length - 1]);
    press("Home");
    expect(document.activeElement).toBe(tabs()[0]);
    // And it wraps rather than dead-ending at the edge.
    press("ArrowLeft");
    expect(document.activeElement).toBe(tabs()[tabs().length - 1]);
  });

  it("opens a tab from the keyboard with Enter", () => {
    act(() => {
      useAppStore.setState({ itemTabs: files("a.md", "b.md") });
    });
    renderStrip();
    act(() => {
      itemTabs()[1].dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(useAppStore.getState().activeItemId).toBe(`file:${FOLDER}\\b.md`);
  });

  it("gives each item kind its own glyph, so a path is not read as an artifact", () => {
    act(() => {
      useAppStore.setState({
        itemTabs: [...files("a.md"), { kind: "artifact", id: "art1", conversationId: "c1" }],
        artifacts: { c1: [{ id: "art1", title: "Poster", kind: "markdown", content: "" } as never] },
      });
    });
    renderStrip();
    expect(itemTabs()[0].className, "a path is mono").toContain("ts-mono");
    expect(itemTabs()[1].className).not.toContain("ts-mono");
    expect(tabs().every((t) => t.querySelector(".ts-tab-icon svg"))).toBe(true);
  });
});

describe("coding tabs (PRJ-UI-2/PRJ-UI-3)", () => {
  const PATH = "C:\\work\\src\\main.rs";
  const change = (): FileChange => ({
    path: PATH,
    display: "src/main.rs",
    status: "modified",
    from: null,
    added: 5,
    removed: 2,
    hunks: [],
    binary: false,
    too_large: false,
    entry_ids: [],
    last_at: 0,
  });
  const changed = (): ChangeSet => ({ files: [change()], added: 5, removed: 2, since: 0, this_run: true });
  const itemTab = () => container.querySelector<HTMLElement>(".ts-tab:not(.ts-session)")!;

  it("labels a diff tab with the file name and puts the counts in its tooltip", () => {
    act(() => {
      useAppStore.setState({
        itemTabs: [{ kind: "diff", id: PATH, conversationId: "c1" }],
        changeSets: { c1: changed() },
      });
    });
    renderStrip();
    expect(itemTab().querySelector(".ts-tab-label")?.textContent).toBe("main.rs");
    expect(itemTab().getAttribute("title") ?? itemTab().querySelector("[title]")?.getAttribute("title")).toContain(
      "+5 / −2"
    );
  });

  it("puts a dot where a changed file's close button would be, and the dot shows the patch", () => {
    act(() => {
      useAppStore.setState({
        itemTabs: [{ kind: "file", id: PATH, conversationId: "c1" }],
        changeSets: { c1: changed() },
        dockView: "files",
      });
    });
    renderStrip();
    const dot = itemTab().querySelector<HTMLButtonElement>(".ts-dirty")!;
    expect(dot).not.toBeNull();
    act(() => dot.click());
    const s = useAppStore.getState();
    expect(s.dockView).toBe("changes");
    expect(s.itemTabs, "the dot does not close the tab").toHaveLength(1);
  });

  it("reads the dot from the tab's own chat, not the live one", () => {
    act(() => {
      useAppStore.setState({
        activeConversationId: "c2",
        itemTabs: [{ kind: "file", id: PATH, conversationId: "c2" }],
        changeSets: { c1: changed() },
      });
    });
    renderStrip();
    expect(itemTab().querySelector(".ts-dirty")).toBeNull();
  });
});
