/**
 * @vitest-environment jsdom
 *
 * The Workbench's three places are tabs, and the panel follows the agent.
 *
 * The follow behaviour is the part worth pinning: it must react to a
 * *transition* (browsing started, an artifact appeared) and not to a steady
 * state, or the panel yanks itself back every render while the user is trying
 * to read a different tab.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Workbench from "./Workbench";
import { useAppStore } from "../../lib/store";
import type { Artifact, BrowserPanelState } from "../../lib/api";
import type { Conversation } from "../../lib/types";

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
    selected: null,
    ...over,
  });
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

const tabs = () => Array.from(container.querySelectorAll(".wb-tab"));
const tabLabels = () => tabs().map((t) => (t.textContent ?? "").trim());
const activeTab = () => container.querySelector(".wb-tab.active")?.textContent?.trim() ?? "";

describe("Workbench tabs", () => {
  it("shows Files and Artifacts when there's a folder, and no Browser tab without a session", () => {
    seed();
    render();
    expect(tabLabels().some((l) => l.startsWith("Files"))).toBe(true);
    expect(tabLabels().some((l) => l.startsWith("Artifacts"))).toBe(true);
    expect(tabLabels().some((l) => l.startsWith("Browser"))).toBe(false);
  });

  it("opens on Files rather than an empty Artifacts", () => {
    seed();
    render();
    expect(activeTab()).toContain("Files");
  });

  it("gains a Browser tab when a session exists", () => {
    seed({ browserSessions: { [CONV]: session() } });
    render();
    expect(tabLabels().some((l) => l.startsWith("Browser"))).toBe(true);
  });

  it("follows the agent to Browser when browsing starts", () => {
    seed();
    render();
    expect(activeTab()).toContain("Files");

    act(() => {
      useAppStore.setState({ browserSessions: { [CONV]: session() } });
    });
    expect(activeTab()).toContain("Browser");
  });

  it("follows the agent to Artifacts when an artifact appears", () => {
    seed();
    render();
    act(() => {
      useAppStore.setState({ artifacts: { [CONV]: [artifact("a1")] } });
    });
    expect(activeTab()).toContain("Artifacts");
  });

  it("leaves the user's chosen tab alone while nothing new happens", () => {
    seed({ browserSessions: { [CONV]: session() } });
    render();
    // The user goes to read Files while the browser session stays open.
    const files = tabs().find((t) => (t.textContent ?? "").startsWith("Files"))!;
    act(() => {
      files.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(activeTab()).toContain("Files");

    // An unrelated re-render with the session still live must not steal it back.
    act(() => {
      useAppStore.setState({ browserSessions: { [CONV]: session({ title: "Example 2" }) } });
    });
    expect(activeTab(), "a steady live session is not a transition").toContain("Files");
  });

  it("falls back to a real tab when the active one disappears", () => {
    seed({ browserSessions: { [CONV]: session() } });
    render();
    // Select Browser deliberately — a session that was already open at first
    // render is not a transition, so nothing auto-selected it.
    const browser = tabs().find((t) => (t.textContent ?? "").startsWith("Browser"))!;
    act(() => {
      browser.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(activeTab()).toContain("Browser");

    // Panel dismissed: the Browser tab goes away underneath the selection.
    act(() => {
      useAppStore.setState({ browserSessions: {} });
    });
    expect(container.querySelector(".wb-tabpanel")).not.toBeNull();
    expect(tabLabels().some((l) => l.startsWith("Browser"))).toBe(false);
    expect(activeTab(), "must land somewhere real, not on a dead tab").not.toBe("");
  });

  it("shows a finished session's record without jumping to it", () => {
    // Re-opening a chat that browsed earlier: the record comes back `closed`.
    // The tab must exist — the panel used to be empty here, beside a
    // transcript full of visits — but nothing new is happening, so the user
    // is not yanked away from wherever they were.
    seed();
    render();
    expect(activeTab()).toContain("Files");

    act(() => {
      useAppStore.setState({
        browserSessions: { [CONV]: session({ closed: true }) },
      });
    });
    expect(tabLabels().some((l) => l.startsWith("Browser"))).toBe(true);
    expect(activeTab(), "a past session is not activity").toContain("Files");
  });

  it("does not follow across a conversation switch", () => {
    seed();
    render();
    expect(activeTab()).toContain("Files");

    // Another chat that happens to *already* hold artifacts is not this chat
    // producing one — the count jump must not be read as activity.
    act(() => {
      useAppStore.setState({
        conversations: [conversation(), conversation({ id: "conv-2" })],
        activeConversationId: "conv-2",
        artifacts: { "conv-2": [artifact("a1"), artifact("a2")] },
      });
    });
    expect(activeTab(), "switching chats is not the agent making something").toContain("Files");
  });
});
