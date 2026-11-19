/**
 * @vitest-environment jsdom
 *
 * `TEAM-6`: the sidebar says who leads a chat only when it is not Poiesis, and can list
 * just the chats one leader leads.
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

const setup = (personas: unknown[], conversations: Conversation[], over: Record<string, unknown> = {}) =>
  useAppStore.setState({
    personas,
    conversations,
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
    ...over,
  } as never);

const mira = { id: "p1", name: "Mira" };
const atlas = { id: "p2", name: "Atlas" };

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () => act(() => root.render(<Rail />));
const titles = () => Array.from(container.querySelectorAll(".chat-row .chat-title")).map((t) => t.textContent);
const row = (title: string) =>
  Array.from(container.querySelectorAll<HTMLElement>(".chat-row")).find((li) => li.querySelector(".chat-title")?.textContent === title)!;
const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const openMenu = () => click(container.querySelector('[aria-label="More chat options"]'));
const leaderItem = (name: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>(".rail-leader-item")).find((b) => b.querySelector(".row-menu-label")?.textContent === name)!;
const item = (text: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>(".rail-filter-menu button")).find((b) => b.textContent?.startsWith(text));

describe("the marker", () => {
  it("shows the persona's face and says who leads, on a chat a persona leads", () => {
    setup([mira], [conv("draft", { leader: "persona:p1", personaId: "p1" })]);
    render();
    const m = row("draft").querySelector(".chat-leader")!;
    expect(m.querySelector(".persona-glyph")?.textContent).toBe("M");
    expect(m.getAttribute("title")).toBe("Led by Mira");
    expect(m.getAttribute("aria-label")).toBe("Led by Mira");
  });

  it("reads an older chat that has only a persona", () => {
    setup([mira], [conv("old", { leader: null, personaId: "p1" })]);
    render();
    expect(row("old").querySelector(".chat-leader")?.getAttribute("title")).toBe("Led by Mira");
  });

  it("is not there for Poiesis, with or without the word, nor for a persona that is gone", () => {
    setup([mira], [conv("a", { leader: "poiesis" }), conv("b", { leader: null }), conv("c", { leader: "persona:gone", personaId: "gone" })]);
    render();
    expect(container.querySelector(".chat-leader")).toBeNull();
  });

  it("comes before the title, and the workspace mark after it", () => {
    setup([mira], [conv("w", { leader: "persona:p1", personaId: "p1", workspace: true })]);
    render();
    const kids = Array.from(row("w").children).map((c) => c.className.split(" ")[0]);
    expect(kids.slice(0, 3)).toEqual(["chat-leader", "chat-title", "chat-workspace"]);
  });
});

describe("the filter", () => {
  const chats = () => [
    conv("with Mira", { leader: "persona:p1", personaId: "p1" }),
    conv("also Mira", { leader: null, personaId: "p1" }),
    conv("with Atlas", { leader: "persona:p2", personaId: "p2" }),
    conv("plain", { leader: null }),
  ];

  it("has no ⋯ for the list until there is a persona", () => {
    setup([], chats());
    render();
    expect(container.querySelector('[aria-label="More chat options"]')).toBeNull();
    expect(container.querySelector(".rail-filter")).toBeNull();
  });

  it("lists the leaders with how many chats each leads", () => {
    setup([mira, atlas], chats());
    render();
    openMenu();
    click(item("Show chats led by"));
    const rows = Array.from(container.querySelectorAll(".rail-leader-item")).map(
      (b) => `${b.querySelector(".row-menu-label")?.textContent} ${b.querySelector(".row-stamp")?.textContent}`
    );
    expect(rows).toEqual(["Poiesis 1", "Mira 2", "Atlas 1"]);
  });

  it("shows only that leader's chats, with one removable line saying so", () => {
    setup([mira, atlas], chats());
    render();
    openMenu();
    click(item("Show chats led by"));
    click(leaderItem("Mira"));
    expect(titles().sort()).toEqual(["also Mira", "with Mira"]);
    const chip = container.querySelector(".rail-filter-chip")!;
    expect(chip.textContent).toContain("Led by Mira");
    click(chip);
    expect(titles()).toHaveLength(4);
    expect(container.querySelector(".rail-filter-chip")).toBeNull();
  });

  it("can show the chats Poiesis leads", () => {
    setup([mira, atlas], chats(), { leaderFilter: "poiesis" });
    render();
    expect(titles()).toEqual(["plain"]);
    expect(container.querySelector(".rail-filter-chip")?.textContent).toContain("Led by Poiesis");
  });

  it("offers no leader that leads nothing", () => {
    setup([mira, atlas], [conv("only", { leader: "persona:p1", personaId: "p1" })]);
    render();
    openMenu();
    click(item("Show chats led by"));
    expect(leaderItem("Atlas").disabled).toBe(true);
    expect(leaderItem("Mira").disabled).toBe(false);
  });

  it("is off when the persona it names is gone", () => {
    setup([mira], chats(), { leaderFilter: "persona:deleted" });
    render();
    expect(titles()).toHaveLength(4);
    expect(container.querySelector(".rail-filter-chip")).toBeNull();
  });

  it("leaves out a project with none of the chats it shows", () => {
    setup([mira], [conv("in shop", { projectId: "s" }), conv("with Mira", { leader: "persona:p1", personaId: "p1" })], {
      projects: [{ id: "s", name: "Shop", rootPath: null }],
      expandedProjects: ["s"],
      leaderFilter: "persona:p1",
    });
    render();
    expect(container.querySelector(".project-row")).toBeNull();
    expect(titles()).toEqual(["with Mira"]);
  });

  it("is what a persona's link sets, and opens a collapsed sidebar", () => {
    setup([mira], chats(), { railCollapsed: true });
    act(() => useAppStore.getState().showChatsLedBy("persona:p1"));
    expect(useAppStore.getState().leaderFilter).toBe("persona:p1");
    expect(useAppStore.getState().railCollapsed).toBe(false);
  });
});
