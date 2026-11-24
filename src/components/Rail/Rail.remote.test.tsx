/**
 * @vitest-environment jsdom
 *
 * `REM-UI-3`: a chat a phone started wears a small phone in the sidebar, and its tooltip names the
 * phone. A chat typed here wears nothing.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import Rail from "./Rail";
import { useAppStore } from "../../lib/store";
import type { Conversation } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const conv = (id: string, over: Partial<Conversation> = {}): Conversation => ({
  id,
  title: id,
  updatedAt: Date.now() - 60_000,
  messages: [],
  projectId: null,
  ...over,
});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({
    personas: [],
    teams: [],
    conversations: [
      conv("typed"),
      conv("from-phone", { remoteSender: "Pixel", remoteTransport: "phone" }),
      conv("from-telegram", { remoteSender: "Erich", remoteTransport: "telegram" }),
    ],
    projects: [],
    expandedProjects: [],
    activeConversationId: null,
    view: "chat",
    railCollapsed: false,
    changeProposals: [],
    reflectingIds: [],
    digestedIds: [],
    runningJob: null,
    paletteOpen: false,
    leaderFilter: null,
  } as never);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const row = (title: string) =>
  Array.from(container.querySelectorAll(".chat-row")).find((r) => r.querySelector(".chat-title")?.textContent === title)!;
const mark = (title: string) => row(title).querySelector<HTMLElement>("[role=img][aria-label^='Started from']");

describe("a chat that came from outside", () => {
  it("names where it came from, and only for those chats", () => {
    act(() => root.render(<Rail />));
    expect(mark("typed")).toBeNull();
    expect(mark("from-phone")!.getAttribute("title")).toBe("Started from Pixel");
    expect(mark("from-telegram")!.getAttribute("title")).toBe("Started from Erich");
    // Two different glyphs: a phone and a paper plane.
    expect(mark("from-phone")!.innerHTML).not.toBe(mark("from-telegram")!.innerHTML);
  });
});
