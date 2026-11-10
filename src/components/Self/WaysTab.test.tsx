/**
 * @vitest-environment jsdom
 *
 * `WRK-16`, `WRK-17`: Self → How I work. A style is picked from cards and shown with its
 * rules; every rule is one sentence made of two fixed lists; a switch turns it off and a
 * ⋯ narrows or switches it off; a style of your own can be made, renamed, deleted (after
 * being told where it is in use), and brought back.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WaysView } from "../../lib/api";

const api = vi.hoisted(() => ({
  waysView: vi.fn(),
  addWay: vi.fn(),
  updateWay: vi.fn(),
  setPresetRule: vi.fn(),
  updatePreset: vi.fn(),
  deletePreset: vi.fn(),
  restorePreset: vi.fn(),
  setStyleDefault: vi.fn(),
}));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import WaysTab from "./WaysTab";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const view = (over: Partial<WaysView> = {}): WaysView => ({
  narrative: "I've changed how I work 2 times since November.",
  ways: [
    {
      slug: "stop-the-line",
      text: "When a check fails, I fix that before anything else.",
      when: "check-fails",
      move: "stop-the-line",
      scope: "everywhere",
      scope_label: "Everywhere",
      status: "always",
      origin: "shipped",
      since: "2026-11-07",
      evidence: [],
    },
    {
      slug: "before-answer-second-look",
      text: "When I'm about to answer, I get a second look.",
      when: "before-answer",
      move: "second-look",
      scope: "project:nexus",
      scope_label: "Project: Nexus",
      status: "learned",
      origin: "you",
      since: "2026-11-07",
      evidence: [{ id: "c9", title: "The export fix" }],
    },
    {
      slug: "old",
      text: "When my head is getting full, I start fresh with a handover note.",
      when: "head-full",
      move: "fresh-start",
      scope: "everywhere",
      scope_label: "Everywhere",
      status: "retired",
      origin: "you",
      since: "2026-10-28",
      evidence: [],
    },
  ],
  presets: [
    { slug: "ad-hoc", name: "Ad hoc", line: "Just answer", glyph: "ad-hoc", built_in: true, locked: false, version: 1, rules: [] },
    { slug: "lean", name: "Lean", line: "One thing at a time", glyph: "lean", built_in: true, locked: false, version: 3, rules: [] },
    { slug: "mine", name: "Mine", line: "My way", glyph: "lean", built_in: false, locked: true, version: 1, rules: [] },
  ],
  preset: "lean",
  rules: [
    { when: "start", move: "one-at-a-time", text: "When I start, I finish one thing before the next.", on: true, original: true },
    { when: "many-changes", move: "split", text: "When I've changed many files, I split it into smaller pieces.", on: false, original: true },
    { when: "risky-action", move: "advice", text: "When I'm about to delete, send or change something in an app, I ask for advice first.", on: true, original: false },
  ],
  global_default: "ad-hoc",
  defaults: [],
  deleted: [],
  proposals: [],
  moments: [],
  recent: [],
  triggers: [
    { value: "start", label: "I start" },
    { value: "before-answer", label: "I'm about to answer" },
  ],
  moves: [
    { value: "one-at-a-time", label: "I finish one thing before the next" },
    { value: "second-look", label: "I get a second look" },
  ],
  scopes: [
    { value: "everywhere", label: "Everywhere" },
    { value: "project:nexus", label: "Project: Nexus" },
    { value: "kind:mail", label: "Kind of work: Mail" },
  ],
  kinds: [],
  ...over,
});

let container: HTMLDivElement;
let root: Root;
const openPresetDialog = vi.fn();
const loadPresets = vi.fn();
const setActiveConversation = vi.fn();

async function show(v: WaysView = view()) {
  api.waysView.mockResolvedValue(v);
  await act(async () => root.render(<WaysTab />));
  await act(async () => {});
}

const buttons = () => [...container.querySelectorAll<HTMLButtonElement>("button")];
const byText = (text: string) => buttons().find((b) => b.textContent?.trim() === text)!;
const byLabel = (label: string) => container.querySelector<HTMLElement>(`[aria-label="${label}"]`)!;
const click = async (el: Element) => act(async () => (el as HTMLElement).click());

beforeEach(() => {
  Object.values(api).forEach((f) => f.mockReset().mockResolvedValue(undefined));
  openPresetDialog.mockReset();
  loadPresets.mockReset().mockResolvedValue(undefined);
  setActiveConversation.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  useAppStore.setState({ presets: [], openPresetDialog, loadPresets, setActiveConversation } as never);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the story and the lists", () => {
  it("says what changed in the first person, then lists the ways by how they stand", async () => {
    await show();
    expect(container.querySelector(".ways-story")?.textContent).toBe("I've changed how I work 2 times since November.");
    const heads = [...container.querySelectorAll(".ways-heading h3")].map((h) => h.textContent);
    expect(heads).toEqual(["Styles", "Rules in every style", "A different style for one place"]);
    expect(container.querySelector(".ways-style.on")?.textContent).toContain("Lean");
    expect(container.querySelector(".ways-detail h3")?.textContent).toBe("Lean");
    // The switched-off rule is folded away, not gone.
    expect(container.querySelector(".ways-fold summary")?.textContent).toBe("Switched off (1)");
    expect(container.textContent).toContain("When a check fails, I fix that before anything else.");
    expect(container.textContent).toContain("Lean, 3rd version");
    expect(container.textContent).toContain("Project: Nexus");
    expect(container.textContent).toContain("Your own, added 2026-11-07.");
  });

  it("shows what a preset has switched off, struck through, and what was added to it", async () => {
    await show();
    const struck = container.querySelector(".ways-struck");
    expect(struck?.textContent).toContain("I split it into smaller pieces");
    expect(container.querySelector(".ways-tag")).toBeTruthy();
    expect(container.textContent).toContain("added");
  });

  it("hides the preset's own list for Ad hoc, which only uses the base rules", async () => {
    await show(view({ preset: "ad-hoc", rules: [] }));
    expect(container.querySelector(".ways-subhead")).toBeNull();
    expect(container.textContent).toContain("Ad hoc has no rules of its own.");
  });
});

describe("changing a way", () => {
  it("switching a base rule off retires it, and the list is read again", async () => {
    await show();
    await click(byLabel("When a check fails, I fix that before anything else."));
    expect(api.updateWay).toHaveBeenCalledWith("stop-the-line", { status: "retired" });
    expect(loadPresets).toHaveBeenCalled();
    expect(api.waysView).toHaveBeenCalledTimes(2);
  });

  it("switching a preset rule on or off changes that preset", async () => {
    await show();
    await click(byLabel("When I've changed many files, I split it into smaller pieces."));
    expect(api.setPresetRule).toHaveBeenCalledWith("lean", { when: "many-changes", move: "split" }, true);
  });

  it("brings a retired way back from its ⋯ menu", async () => {
    await show();
    await click(byLabel("More for: When my head is getting full, I start fresh with a handover note."));
    await click(byText("Bring back"));
    expect(api.updateWay).toHaveBeenCalledWith("old", { status: "learned" });
  });

  it("narrows a way to where it applies, from the lists the backend gave", async () => {
    await show();
    await click(byLabel("More for: When I'm about to answer, I get a second look."));
    await click(byText("Only in…"));
    expect([...container.querySelectorAll(".row-menu-item")].map((b) => b.textContent)).toEqual([
      "Everywhere",
      "Project: Nexus✓",
      "Kind of work: Mail",
    ]);
    await click([...container.querySelectorAll(".row-menu-item")][2]);
    expect(api.updateWay).toHaveBeenCalledWith("before-answer-second-look", { scope: "kind:mail" });
  });

  it("opens the chat a way came from", async () => {
    await show();
    await click(byLabel("More for: When I'm about to answer, I get a second look."));
    await click(byText("Where it came from: The export fix"));
    expect(setActiveConversation).toHaveBeenCalledWith("c9");
  });

  it("adds a way from two lists and a reach, never from free text", async () => {
    await show();
    await click(byText("+ Add a rule for every style"));
    expect(container.querySelector("input[type=text]")).toBeNull();
    await click(byText("Add"));
    expect(api.addWay).toHaveBeenCalledWith("start", "one-at-a-time", "everywhere");
  });

  it("says plainly when the backend refuses", async () => {
    api.addWay.mockRejectedValue("That way of working is already there.");
    await show();
    await click(byText("+ Add a rule for every style"));
    await click(byText("Add"));
    expect(container.querySelector(".ways-problem")?.textContent).toBe("That way of working is already there.");
  });
});

describe("presets", () => {
  it("makes a preset of your own from the one shown", async () => {
    await show();
    await click(byLabel("More for Lean"));
    await click(byText("Make my own from it…"));
    expect(openPresetDialog).toHaveBeenCalledWith(expect.objectContaining({ mode: "new", from: "lean", name: "", line: "" }));
  });

  it("makes the one shown the start of new chats, and locks or unlocks it", async () => {
    await show();
    await click(byLabel("New chats start in this style"));
    expect(api.setStyleDefault).toHaveBeenCalledWith("global", "lean");
    await click(byLabel("I may improve it"));
    expect(api.updatePreset).toHaveBeenCalledWith("lean", { locked: true });
  });

  it("a built-in style can be copied but not renamed or deleted", async () => {
    await show();
    await click(byLabel("More for Lean"));
    expect([...container.querySelectorAll(".row-menu-item")].map((b) => b.textContent)).toEqual(["Make my own from it…"]);
  });

  it("shows another style when its card is picked", async () => {
    await show();
    await click(container.querySelector<HTMLElement>('.ways-style[role="radio"]')!);
    expect(api.waysView).toHaveBeenLastCalledWith("ad-hoc");
  });

  it("starts a new style from the card at the end of the list", async () => {
    await show();
    await click(byText("+New styleFrom scratch or from one of these"));
    expect(openPresetDialog).toHaveBeenCalledWith(expect.objectContaining({ mode: "new", from: "lean" }));
  });

  it("a preset of your own has a menu to rename, duplicate or delete", async () => {
    await show(view({ preset: "mine" }));
    await click(byLabel("More for Mine"));
    expect([...container.querySelectorAll(".row-menu-item")].map((b) => b.textContent)).toEqual(["Rename", "Duplicate", "Delete"]);
    await click(byText("Rename"));
    expect(openPresetDialog).toHaveBeenCalledWith({ mode: "rename", slug: "mine", name: "Mine", line: "My way" });
  });

  it("asks before deleting a preset that is the default somewhere, and names where", async () => {
    api.deletePreset.mockResolvedValueOnce({ deleted: false, used_by: ["new chats", "the project nexus"] });
    await show(view({ preset: "mine" }));
    await click(byLabel("More for Mine"));
    await click(byText("Delete"));
    expect(api.deletePreset).toHaveBeenCalledWith("mine", false);
    expect(document.body.textContent).toContain(`"Mine" is the default for new chats and the project nexus. Those go back to Ad hoc.`);
    api.deletePreset.mockResolvedValueOnce({ deleted: true, used_by: [] });
    await click([...document.querySelectorAll<HTMLButtonElement>(".confirm-go")][0]);
    expect(api.deletePreset).toHaveBeenLastCalledWith("mine", true);
  });

  it("deletes one that is nowhere in use at once, and lists deleted ones to bring back", async () => {
    api.deletePreset.mockResolvedValueOnce({ deleted: true, used_by: [] });
    await show(view({ preset: "mine", deleted: [{ file: "1-preset-old.md", name: "Old one", at: 1 }] }));
    await click(byLabel("More for Mine"));
    await click(byText("Delete"));
    expect(document.querySelector(".confirm-card")).toBeNull();
    expect(container.textContent).toContain("Old one");
    await click(byText("Bring back"));
    expect(api.restorePreset).toHaveBeenCalledWith("1-preset-old.md");
  });

  it("sets where a project, persona or kind of work starts", async () => {
    await show();
    await click(byText("Set"));
    expect(api.setStyleDefault).toHaveBeenCalledWith("project:nexus", "lean");
  });
});
