/**
 * @vitest-environment jsdom
 *
 * `WRK-3`, `WHY-4`, `CMP-7`: the "this chat" button under the composer. It draws
 * how I work as its glyph, Ad hoc too, and opens one menu with how I work, what
 * I'm working from, and the standing modes Workspace and Persona.
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
    const how = items()[0];
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
    expect(items()[0].title).toContain("Your choice for this chat");
  });

  it("opens what I'm working from on the live view", () => {
    put(null);
    act(() => root.render(<ChatSetup />));
    open();
    act(() => items()[1].click());
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
