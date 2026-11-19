/**
 * @vitest-environment jsdom
 *
 * `TEAM-5`: what a persona knows. Its own entries (facts and lessons) are listed, each can be
 * shared with everyone or forgotten; "Remember across chats" switches it off at once.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Fact, Persona } from "../../lib/api";

const api = vi.hoisted(() => ({
  listMemoryFacts: vi.fn(),
  listLessons: vi.fn(),
  shareMemory: vi.fn(),
  forgetMemoryFact: vi.fn(),
  forgetLesson: vi.fn(),
  updatePersona: vi.fn(),
}));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import PersonaKnows from "./PersonaKnows";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const mira = { id: "p1", name: "Mira", remembers: true } as Persona;
const entry = (name: string, description: string, persona: string | null): Fact =>
  ({ name, description, kind: "preference", created: "2026-11-01", source_conversation: null, body: description, scope: "topical", persona }) as Fact;

let container: HTMLDivElement;
let root: Root;
const refreshMemoryContext = vi.fn();
const openSelf = vi.fn();

beforeEach(() => {
  api.listMemoryFacts
    .mockReset()
    .mockResolvedValue([
      entry("short-intros", "Likes short intros in newsletters", "p1"),
      entry("company-name", "The company is Acme", null),
      entry("long-intros", "Likes long intros", "p2"),
    ]);
  api.listLessons.mockReset().mockResolvedValue([entry("warm-opener", "Open warmly", "p1")]);
  api.shareMemory.mockReset().mockResolvedValue(undefined);
  api.forgetMemoryFact.mockReset().mockResolvedValue("file");
  api.forgetLesson.mockReset().mockResolvedValue("file");
  api.updatePersona.mockReset().mockResolvedValue(undefined);
  refreshMemoryContext.mockReset().mockResolvedValue(undefined);
  openSelf.mockReset();
  useAppStore.setState({
    personas: [mira],
    refreshMemoryContext,
    openSelf,
    refreshPersonas: () => Promise.resolve(),
  } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = async (p: Persona = mira) => {
  await act(async () => root.render(<PersonaKnows persona={p} />));
  await act(async () => void (await Promise.resolve()));
};
const rows = () => Array.from(container.querySelectorAll(".persona-knows-row"));
const press = (text: string, within: ParentNode = container) =>
  act(async () => {
    Array.from(within.querySelectorAll("button")).find((b) => b.textContent === text)!.click();
  });
/** An entry's own ⋯ menu, opened, then one of its items. */
const pressIn = async (row: Element, text: string) => {
  await act(async () => row.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!.click());
  await press(text, row);
};

describe("what Mira knows", () => {
  it("lists only her own facts and lessons, not what everyone knows or another persona does", async () => {
    await render();
    expect(rows().map((r) => r.querySelector(".persona-knows-text")!.textContent)).toEqual([
      "Likes short intros in newsletters",
      "Open warmly · lesson",
    ]);
  });

  it("leaves out an earlier version that a newer one replaced", async () => {
    api.listMemoryFacts.mockResolvedValue([
      { ...entry("intros", "Likes short intros", "p1"), status: "superseded", superseded_by: "intros-2" },
      entry("intros-2", "Likes very short intros", "p1"),
    ]);
    await render();
    expect(rows().map((r) => r.querySelector(".persona-knows-text")!.textContent)).toEqual([
      "Likes very short intros",
      "Open warmly · lesson",
    ]);
  });

  it("shares an entry with everyone, which takes it off her list", async () => {
    await render();
    api.listMemoryFacts.mockResolvedValue([entry("short-intros", "Likes short intros in newsletters", null)]);
    await pressIn(rows()[0], "Share with everyone");
    expect(api.shareMemory).toHaveBeenCalledWith("facts", "short-intros");
    expect(rows().map((r) => r.textContent)).not.toContain("Likes short intros in newsletters");
    expect(refreshMemoryContext).toHaveBeenCalled();
  });

  it("forgets a fact and a lesson, each through its own door", async () => {
    await render();
    await pressIn(rows()[0], "Forget");
    expect(api.forgetMemoryFact).toHaveBeenCalledWith("short-intros");
    await pressIn(rows()[1], "Forget");
    expect(api.forgetLesson).toHaveBeenCalledWith("warm-opener");
  });

  it("says so when there is nothing yet", async () => {
    api.listMemoryFacts.mockResolvedValue([]);
    api.listLessons.mockResolvedValue([]);
    await render();
    expect(rows()).toHaveLength(0);
    expect(container.textContent).toContain("Nothing yet.");
  });

  it("is on until switched off, and switching writes at once", async () => {
    await render();
    const box = container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(box.checked).toBe(true);
    await act(async () => box.click());
    expect(api.updatePersona).toHaveBeenCalledWith(expect.objectContaining({ id: "p1", remembers: false }));
  });

  it("reads a persona from before the switch as one that remembers, and says what off means", async () => {
    await render({ ...mira, remembers: undefined });
    expect(container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
    await render({ ...mira, remembers: false });
    expect(container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(false);
    expect(container.textContent).toContain("Mira keeps nothing and sees only what everyone knows.");
  });

  it("opens Memory on her entries", async () => {
    await render();
    await press("See it in Memory");
    expect(openSelf).toHaveBeenCalledWith("memory", "persona:p1");
  });
});
