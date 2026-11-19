/**
 * @vitest-environment jsdom
 *
 * `WRK-3`, `WHY-4`, `CMP-7`: the "this chat" button under the composer. It draws
 * how I work as its glyph, Ad hoc too, and opens one menu with who leads, how I
 * work, what I'm working from, and the standing mode Workspace.
 *
 * `TEAM-2`: when a persona leads, the button draws its letter with the way of
 * working as a small badge, and says who leads in its tooltip.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChatSetup from "./ChatSetup";
import ModeChips from "./ModeChips";
import ModifierChips from "./ModifierChips";
import { useAppStore } from "../../lib/store";
import type { PresetView } from "../../lib/api";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const requestComposer = vi.fn();
const openContextPanel = vi.fn();
let container: HTMLDivElement;
let root: Root;

const PRESETS: PresetView[] = [
  { slug: "ad-hoc", name: "Ad hoc", line: "Just answer", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
  {
    slug: "lean",
    name: "Lean",
    line: "One thing at a time, stop the line when something breaks",
    glyph: "lean",
    built_in: true,
    locked: false,
    version: 1,
    rules: [],
  },
];

function put(style: string | null, extra: Record<string, unknown> = {}) {
  act(() =>
    useAppStore.setState({
      activeConversationId: "c1",
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style }],
      presets: PRESETS,
      requestComposer,
      openContextPanel,
      turnModifiers: {},
      workspaceMode: false,
      toolsEnabled: true,
      ...extra,
    } as never)
  );
}

beforeEach(() => {
  requestComposer.mockClear();
  openContextPanel.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const trigger = () => container.querySelector<HTMLButtonElement>(".chat-setup-btn");
const items = () => Array.from(container.querySelectorAll<HTMLButtonElement>(".chat-setup-menu [role=menuitem]"));
const open = () => act(() => trigger()!.click());

describe("the this-chat button", () => {
  it("is an icon button like + and /, there for Ad hoc too, and quiet", () => {
    put(null);
    act(() => root.render(<ChatSetup />));
    expect(trigger()?.className).toContain("icon-btn");
    expect(trigger()?.className).not.toContain("lit");
    expect(trigger()?.textContent).toBe("");
    expect(trigger()?.title).toContain("How I work: Ad hoc");
  });

  it("draws the preset's glyph and is lit when one is chosen", () => {
    put("lean");
    act(() => root.render(<ChatSetup />));
    expect(trigger()?.querySelector("svg.glyph-lean")).not.toBeNull();
    expect(trigger()?.className).toContain("lit");
    expect(trigger()?.title).toContain("How I work: Lean");
  });

  it("reads as Ad hoc before the list of presets has arrived", () => {
    put(null, { presets: [] });
    act(() => root.render(<ChatSetup />));
    expect(trigger()?.title).toContain("How I work: Ad hoc");
  });
});

describe("the this-chat menu", () => {
  it("says how I work and where that came from, and opens /style", () => {
    put(null);
    act(() => root.render(<ChatSetup />));
    open();
    const how = items()[1];
    expect(how.textContent).toContain("How I work");
    expect(how.textContent).toContain("Just answer");
    expect(how.title, "nobody chose it, and it does not say they did").toContain("unless you say otherwise");
    act(() => how.click());
    expect(requestComposer).toHaveBeenCalledWith("/style ");
    expect(container.querySelector(".chat-setup-menu")).toBeNull();
  });

  it("says when it is the chat's own choice", () => {
    put("lean");
    act(() => root.render(<ChatSetup />));
    open();
    expect(items()[1].title).toContain("Your choice for this chat");
  });

  it("opens what I'm working from on the live view", () => {
    put(null);
    act(() => root.render(<ChatSetup />));
    open();
    act(() => items()[2].click());
    expect(openContextPanel).toHaveBeenCalledWith({ conversationId: "c1" });
  });

  it("lists Workspace with a way to leave it, and leaves Tools off to the chips", () => {
    put(null, { workspaceMode: true, workspaceShown: false, toolsEnabled: false });
    act(() =>
      root.render(
        <>
          <ChatSetup />
          <ModeChips only={["tools", "tier", "external"]} />
        </>
      )
    );
    expect(trigger()?.className).toContain("lit");
    expect(Array.from(container.querySelectorAll(".mode-chip-label")).map((b) => b.textContent)).toEqual(["Tools off"]);
    open();
    expect(items().map((b) => b.textContent)).toEqual([
      expect.stringContaining("Who leads"),
      expect.stringContaining("How I work"),
      expect.stringContaining("What I'm working from"),
      expect.stringContaining("Workspace"),
    ]);
    act(() => container.querySelector<HTMLButtonElement>(".chat-setup-off")!.click());
    expect(useAppStore.getState().workspaceMode).toBe(false);
  });

  it("closes on Escape", () => {
    put(null);
    act(() => root.render(<ChatSetup />));
    open();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(container.querySelector(".chat-setup-menu")).toBeNull();
  });
});

const MIRA = { id: "p1", name: "Mira" };

describe("who leads (TEAM-2)", () => {
  it("is quiet when Poiesis leads: the style glyph, no 'Led by' line", () => {
    put("lean", { personas: [MIRA] });
    act(() => root.render(<ChatSetup />));
    expect(trigger()?.querySelector("svg.glyph-lean")).not.toBeNull();
    expect(trigger()?.querySelector(".persona-glyph")).toBeNull();
    expect(trigger()?.title).not.toContain("Led by");
    open();
    expect(items()[0].textContent).toContain("Who leads");
    expect(items()[0].textContent).toContain("Poiesis");
  });

  it("draws the persona's letter with the way of working as a badge, and says it in the tooltip", () => {
    put("lean", { personas: [MIRA], conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean", leader: "persona:p1", personaId: "p1" }] });
    act(() => root.render(<ChatSetup />));
    const glyph = trigger()!.querySelector(".persona-glyph")!;
    expect(glyph.textContent).toContain("M");
    expect(glyph.querySelector(".persona-glyph-badge svg.glyph-lean")).not.toBeNull();
    expect(trigger()?.className).toContain("lit");
    expect(trigger()?.title.split("\n")[0]).toBe("Led by Mira");
    expect(trigger()?.title).toContain("How I work: Lean");
    expect(trigger()?.getAttribute("aria-label")).toContain("Led by Mira");
  });

  it("draws a team in its coordinator's hue and names the coordinator in the tooltip (II.2)", () => {
    const shop = {
      slug: "shop",
      name: "Shop team",
      coordinator: "persona:p1",
      members: [
        { role: "api", persona: "persona:p1", job: "", owns: [] },
        { role: "docs", persona: "helper", job: "", owns: [] },
      ],
    };
    put("lean", {
      personas: [MIRA],
      teams: [shop],
      conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: "lean", leader: "team:shop", personaId: "p1" }],
    });
    act(() => root.render(<ChatSetup />));
    expect(trigger()!.querySelector(".team-glyph .persona-glyph-badge svg.glyph-lean")).not.toBeNull();
    expect(trigger()?.title.split("\n")[0]).toBe("Led by Shop team · coordinator Mira, 1 persona");
    open();
    expect(items()[0].textContent).toContain("Shop team");
  });

  it("reads a chat that only has a persona (from before leaders) as led by it", () => {
    put(null, { personas: [MIRA], conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: null, personaId: "p1" }] });
    act(() => root.render(<ChatSetup />));
    expect(trigger()?.title).toContain("Led by Mira");
  });

  it("treats a persona that is gone as nobody: Poiesis leads", () => {
    put(null, { personas: [], conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: null, leader: "persona:gone", personaId: "gone" }] });
    act(() => root.render(<ChatSetup />));
    expect(trigger()?.title).not.toContain("Led by");
  });

  it("opens /lead from the first row, and no longer lists Persona under 'On in this chat'", () => {
    put(null, { personas: [MIRA], workspaceMode: true, conversations: [{ id: "c1", title: "t", updatedAt: 0, messages: [], style: null, leader: "persona:p1", personaId: "p1" }] });
    act(() => root.render(<ChatSetup />));
    open();
    expect(items().map((b) => b.textContent)).not.toContainEqual(expect.stringMatching(/^Persona/));
    act(() => items()[0].click());
    expect(requestComposer).toHaveBeenCalledWith("/lead ");
  });
});

describe("a preset for one message", () => {
  it("shows as a chip that goes with the send, and leaves the chat's own way alone", () => {
    put("lean", { turnModifiers: { style: "ad-hoc" } });
    act(() =>
      root.render(
        <>
          <ModifierChips />
          <ChatSetup />
        </>
      )
    );
    expect(container.querySelector(".modifier-chip-label")?.textContent).toBe("Ad hoc, this once");
    expect(trigger()?.title).toContain("How I work: Lean");
  });
});
