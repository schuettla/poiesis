/**
 * @vitest-environment jsdom
 *
 * `SHL-21`..`SHL-23`: the right sidebar navigates its own sub-views
 * (`dockView` in the store) and the agent may move it there but never focus a
 * tab. Keeps the regressions the older versions of this file guarded against
 * — a section that disappears must not blank the panel, and a fresh chat with
 * a folder lands on Files — plus the follow behaviour's transition-only rule.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Workbench from "./Workbench";
import { useAppStore } from "../../lib/store";
import type { Artifact, BrowserPanelState, ChangeSet, FileChange } from "../../lib/api";
import type { Conversation, SubRun } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollTo = () => {};

const CONV = "conv-1";

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: CONV,
    title: "A chat",
    updatedAt: 0,
    messages: [],
    folderPath: "C:\\work",
    folderTrust: "confirm",
    ...overrides,
  };
}

function artifact(id: string): Artifact {
  return {
    id,
    conversation_id: CONV,
    kind: "markdown",
    title: id,
    content: "hello",
    saved_path: null,
    created_at: 0,
  };
}

function session(overrides: Partial<BrowserPanelState> = {}): BrowserPanelState {
  return {
    domain: "example.com",
    title: "Example",
    screenshot: null,
    trail: ["visited example.com"],
    closed: false,
    ...overrides,
  };
}

function subRun(): SubRun {
  return {
    runId: "r1",
    conversationId: "child-1",
    parentConversationId: CONV,
    agent: "general",
    task: "do a thing",
    status: "running",
    steps: [],
    text: "",
    startedAt: Date.now(),
  };
}

let container: HTMLDivElement;
let root: Root;

/** Only the store shape this component reads; everything else keeps its
 * initial value so the test says what it depends on. */
function seed(over: Partial<ReturnType<typeof useAppStore.getState>> = {}) {
  useAppStore.setState({
    conversations: [conversation()],
    activeConversationId: CONV,
    artifacts: {},
    browserSessions: {},
    subRuns: {},
    selected: null,
    itemTabs: [],
    activeItemId: null,
    dockView: "files",
    changeSets: {},
    changesFocus: null,
    trash: [],
    ...over,
  });
}

function changeSet(paths: string[]): ChangeSet {
  const files: FileChange[] = paths.map((path) => ({
    path,
    display: path.replace("C:\\work\\", ""),
    status: "modified",
    from: null,
    added: 2,
    removed: 1,
    hunks: [],
    binary: false,
    too_large: false,
    entry_ids: [],
    last_at: 0,
  }));
  return { files, added: 2 * files.length, removed: files.length, since: 0, this_run: true };
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () =>
  act(() => {
    root.render(<Workbench />);
  });

const navTabs = () => Array.from(container.querySelectorAll<HTMLElement>(".wb-tab"));
const navLabels = () => navTabs().map((t) => (t.textContent ?? "").trim());
const selectedLabel = () =>
  (navTabs().find((t) => t.getAttribute("aria-selected") === "true")?.textContent ?? "").trim();
const clickNav = (startsWith: string) => {
  const tab = navTabs().find((t) => (t.textContent ?? "").startsWith(startsWith))!;
  act(() => {
    tab.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
};

describe("Workbench sub-views", () => {
  it("falls back to the artifact list, not the tree, without a folder", () => {
    seed({
      conversations: [conversation({ folderPath: null })],
      artifacts: { [CONV]: [artifact("a1")] },
    });
    render();
    expect(container.querySelector(".wb-files")).toBeNull();
    expect(container.querySelector(".wb-artifacts")).not.toBeNull();
  });

  it("offers a folder from the Files section when a folderless chat asks for it", () => {
    seed({ conversations: [conversation({ folderPath: null })] });
    render();
    expect(selectedLabel()).toBe("Artifacts");
    clickNav("Files");
    expect(selectedLabel()).toBe("Files");
    expect(container.textContent).toContain("Give Poiesis a folder to work in");
  });

  // The core sections hold their places so the row can be learned; only the
  // live ones (Agents, Browser) come and go.
  it("lands on Files, with no Browser or Agents section until one exists", () => {
    seed();
    render();
    expect(container.querySelector(".wb-files")).not.toBeNull();
    expect(navLabels()).toEqual(["Files", "Artifacts", "Changes"]);
    expect(selectedLabel()).toBe("Files");
  });

  it("no longer carries the chat's own actions — scheduling lives in the chat's menu", () => {
    seed();
    render();
    expect(container.textContent).not.toContain("Schedule this");
  });

  it("offers a Browser section once a session exists, without switching to it", () => {
    seed({ browserSessions: { [CONV]: session() } });
    render();
    // A session that was already open at first render is not a transition.
    expect(navLabels().some((l) => l.startsWith("Browser"))).toBe(true);
    expect(selectedLabel()).toBe("Files");
  });

  it("switches sections from its own nav", () => {
    seed({ artifacts: { [CONV]: [artifact("a1")] } });
    render();
    clickNav("Artifacts");
    expect(useAppStore.getState().dockView).toBe("artifacts");
    expect(container.querySelector(".wb-artifacts")).not.toBeNull();
    expect(container.querySelector(".wb-files")).toBeNull();
  });

  it("follows the agent to Browser when browsing starts", () => {
    seed();
    render();
    act(() => {
      useAppStore.setState({ browserSessions: { [CONV]: session() } });
    });
    expect(useAppStore.getState().dockView).toBe("browser");
    expect(container.querySelector(".wb-browser")).not.toBeNull();
  });

  it("leaves a manually picked section alone while nothing new happens", () => {
    seed({ browserSessions: { [CONV]: session() } });
    render();
    clickNav("Artifacts");
    act(() => {
      useAppStore.setState({ browserSessions: { [CONV]: session({ title: "Example 2" }) } });
    });
    expect(selectedLabel(), "a steady live session is not a transition").toBe("Artifacts");
  });

  it("falls back to real content when the showing section disappears", () => {
    seed({ browserSessions: { [CONV]: session() } });
    render();
    clickNav("Browser");
    act(() => {
      useAppStore.setState({ browserSessions: {} });
    });
    expect(container.querySelector(".wb-browser")).toBeNull();
    expect(container.querySelector(".wb-files"), "must land somewhere real").not.toBeNull();
  });

  it("shows a finished session's section without jumping to it", () => {
    seed();
    render();
    act(() => {
      useAppStore.setState({ browserSessions: { [CONV]: session({ closed: true }) } });
    });
    expect(navLabels().some((l) => l.startsWith("Browser"))).toBe(true);
    expect(selectedLabel(), "a past session is not activity").toBe("Files");
  });

  // `SHL-23-T`: the one rule about trust. The agent may move the sidebar's
  // overview; it may never take focus off an item the user opened, and it may
  // never change which conversation is live.
  it("moves the sidebar on a subrun starting and touches no tab", () => {
    seed({
      conversations: [conversation(), conversation({ id: "conv-2" })],
      itemTabs: [{ kind: "file", id: "C:\\work\\notes.md" }],
      activeItemId: null,
    });
    render();
    const before = useAppStore.getState();

    act(() => {
      useAppStore.setState({ subRuns: { r1: subRun() } });
    });

    const s = useAppStore.getState();
    expect(s.dockView).toBe("agents");
    expect(s.activeItemId).toBeNull();
    expect(s.itemTabs).toEqual(before.itemTabs);
    expect(s.activeConversationId).toBe(before.activeConversationId);
  });

  // `SHL-24-T`: a row in an overview opens an item tab into the pane beside
  // this one. The list it came from does not move and is not covered — having
  // both on screen is the whole point of giving the item its own column.
  it("opens a run from the Agents list without disturbing the list", () => {
    seed({ subRuns: { r1: subRun() }, dockView: "agents" });
    render();
    const row = container.querySelector<HTMLElement>(".agents-row")!;
    act(() => {
      row.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    const s = useAppStore.getState();
    expect(s.activeItemId).toBe("run:r1");
    expect(s.dockView, "the sidebar stays on its list").toBe("agents");
    // The item renders in its own column, which this panel does not own.
    expect(container.querySelector(".item-view")).toBeNull();
    expect(container.querySelector(".agents-row"), "the list is still there").not.toBeNull();
  });
});

describe("the Changes sub-view (PRJ-UI-3)", () => {
  it("is always offered, with no count until something changed", () => {
    seed();
    render();
    expect(navLabels()).toContain("Changes");
  });

  it("is offered with a count of changed files", () => {
    seed({ changeSets: { [CONV]: changeSet(["C:\\work\\a.rs", "C:\\work\\b.rs"]) } });
    render();
    const tab = navLabels().find((l) => l.startsWith("Changes"));
    expect(tab).toBeDefined();
    expect(tab).toContain("2");
  });

  it("explains itself, and lists no patches, in a chat with no folder", () => {
    seed({
      conversations: [conversation({ folderPath: null })],
      changeSets: { [CONV]: changeSet(["C:\\work\\a.rs"]) },
    });
    render();
    expect(navLabels()).toContain("Changes");
    clickNav("Changes");
    expect(container.querySelector(".chg-path")).toBeNull();
    expect(container.textContent).toContain("No changes to review");
  });

  it("opens one file's patch as a tab without moving the sidebar", () => {
    seed({ changeSets: { [CONV]: changeSet(["C:\\work\\a.rs"]) }, dockView: "changes" });
    render();
    const path = container.querySelector<HTMLButtonElement>(".chg-path")!;
    act(() => path.click());
    const s = useAppStore.getState();
    expect(s.activeItemId).toBe("diff:C:\\work\\a.rs");
    expect(s.itemTabs[0]).toMatchObject({ kind: "diff", conversationId: CONV });
    expect(s.dockView).toBe("changes");
  });

  it("asks before Undo all rather than putting everything back on one click", () => {
    seed({ changeSets: { [CONV]: changeSet(["C:\\work\\a.rs"]) }, dockView: "changes" });
    render();
    const undoAll = Array.from(container.querySelectorAll<HTMLButtonElement>(".chg-head button")).find(
      (b) => b.textContent === "Undo all"
    )!;
    act(() => undoAll.click());
    expect(container.textContent).toContain("Put all 1 file back");
    expect(useAppStore.getState().changeSets[CONV].files).toHaveLength(1);
  });
});
