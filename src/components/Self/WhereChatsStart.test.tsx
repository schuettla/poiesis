/**
 * @vitest-environment jsdom
 *
 * `TEAM-5b`: Where chats start. A row can set who leads, the style, or both; a leader links
 * to its entry in Personas & teams; the style card says which places start in it.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WaysView } from "../../lib/api";

const api = vi.hoisted(() => ({ setLeaderDefault: vi.fn(), setStyleDefault: vi.fn() }));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  ...api,
}));

import WhereChatsStart from "./WhereChatsStart";
import { useAppStore } from "../../lib/store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const view = (over: Partial<WaysView> = {}) =>
  ({
    presets: [
      { slug: "ad-hoc", name: "Ad hoc" },
      { slug: "lean", name: "Lean" },
    ],
    scopes: [
      { value: "everywhere", label: "Everywhere" },
      { value: "project:shop", label: "Project: Shop" },
      { value: "project:news", label: "Project: Newsletter" },
      { value: "persona:p1", label: "Persona: Mira" },
      { value: "kind:mail", label: "Kind of work: Mail" },
    ],
    starts: [
      { scope: "global", label: "Everywhere", style: null, leader: null },
      { scope: "project:news", label: "Project: Newsletter", style: null, leader: "persona:p1" },
      { scope: "persona:p1", label: "Persona: Mira", style: "lean", leader: null },
    ],
    ...over,
  }) as WaysView;

let container: HTMLDivElement;
let root: Root;
const act_ = vi.fn(async (fn: () => Promise<unknown>) => void (await fn()));
const openPersonas = vi.fn();

beforeEach(() => {
  api.setLeaderDefault.mockReset().mockResolvedValue(undefined);
  api.setStyleDefault.mockReset().mockResolvedValue(undefined);
  act_.mockClear();
  openPersonas.mockReset();
  useAppStore.setState({ personas: [{ id: "p1", name: "Mira" }], openPersonas } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = (v = view()) => act(async () => root.render(<WhereChatsStart view={v} act={act_ as never} />));
const rows = () => Array.from(container.querySelectorAll<HTMLElement>(".ways-start"));
const rowOf = (where: string) => rows().find((r) => r.querySelector(".ways-text")?.textContent === where)!;
const choose = async (row: ParentNode, selectLabel: string, option: string) => {
  const trigger = row.querySelector<HTMLElement>(`[aria-label="${selectLabel}"]`) ?? row.querySelector<HTMLElement>(`button[aria-label="${selectLabel}"]`);
  await act(async () => trigger!.click());
  const o = Array.from(container.querySelectorAll<HTMLElement>("[role=option]")).find((x) => x.textContent?.startsWith(option))!;
  await act(async () => o.click());
};

describe("Where chats start", () => {
  it("lists Everywhere first, then each place with who leads and the style", async () => {
    await render();
    expect(rows().map((r) => r.querySelector(".ways-text")!.textContent)).toEqual([
      "Everywhere",
      "Project: Newsletter",
      "Persona: Mira",
    ]);
    expect(rowOf("Project: Newsletter").textContent).toContain("Mira");
    expect(rowOf("Persona: Mira").textContent).toContain("Led by Mira");
  });

  it("sets only who leads, and leaves the style alone", async () => {
    await render();
    await choose(rowOf("Everywhere"), "Who leads at Everywhere", "Poiesis");
    expect(api.setLeaderDefault).toHaveBeenCalledWith("global", "poiesis");
    expect(api.setStyleDefault).not.toHaveBeenCalled();
  });

  it("sets only the style, and leaves who leads alone", async () => {
    await render();
    await choose(rowOf("Project: Newsletter"), "Style at Project: Newsletter", "Lean");
    expect(api.setStyleDefault).toHaveBeenCalledWith("project:news", "lean");
    expect(api.setLeaderDefault).not.toHaveBeenCalled();
  });

  it("calls the empty style the leader's own once somebody leads there, and Not set otherwise", async () => {
    await render();
    const list = (label: string) => container.querySelector(`[role=listbox][aria-label="${label}"]`)?.textContent;
    await act(async () => rowOf("Project: Newsletter").querySelector<HTMLElement>('[aria-label="Style at Project: Newsletter"]')!.click());
    expect(list("Style at Project: Newsletter")).toContain("The leader's style");
    await act(async () => rowOf("Everywhere").querySelector<HTMLElement>('[aria-label="Style at Everywhere"]')!.click());
    expect(list("Style at Everywhere")).toContain("Not set");
  });

  it("opens the leader's entry in Personas & teams", async () => {
    await render();
    await act(async () => rowOf("Project: Newsletter").querySelector<HTMLButtonElement>('[aria-label="Open Mira in Personas & teams"]')!.click());
    expect(openPersonas).toHaveBeenCalledWith("p1");
  });

  it("takes a place away by clearing both, and has no way to remove Everywhere", async () => {
    await render();
    expect(Array.from(rowOf("Everywhere").querySelectorAll("button")).some((b) => b.textContent === "Remove")).toBe(false);
    await act(async () => void Array.from(rowOf("Project: Newsletter").querySelectorAll("button")).find((b) => b.textContent === "Remove")!.click());
    expect(api.setLeaderDefault).toHaveBeenCalledWith("project:news", null);
    expect(api.setStyleDefault).toHaveBeenCalledWith("project:news", null);
  });

  it("a persona's own row has no leader to change and clears only its style", async () => {
    await render();
    const row = rowOf("Persona: Mira");
    expect(row.querySelector('[aria-label="Who leads at Persona: Mira"]')).toBeNull();
    await act(async () => void Array.from(row.querySelectorAll("button")).find((b) => b.textContent === "Remove")!.click());
    expect(api.setLeaderDefault).not.toHaveBeenCalled();
    expect(api.setStyleDefault).toHaveBeenCalledWith("persona:p1", null);
  });

  it("adds a place with a leader only, and only offers places that are not rows yet", async () => {
    await render();
    const add = container.querySelector<HTMLElement>(".ways-addrow")!;
    const setBtn = Array.from(add.querySelectorAll("button")).find((b) => b.textContent === "Set")!;
    expect(setBtn.disabled, "nothing chosen yet").toBe(true);
    await act(async () => add.querySelector<HTMLElement>('[aria-label="For"]')!.click());
    const offered = Array.from(container.querySelectorAll("[role=option]")).map((o) => o.textContent);
    expect(offered.some((o) => o?.includes("Newsletter")), "already a row").toBe(false);
    expect(offered.some((o) => o?.includes("Mira")), "a persona's chats are set on its page").toBe(false);
    await act(async () => void (Array.from(container.querySelectorAll<HTMLElement>("[role=option]")).find((o) => o.textContent?.includes("Shop"))!.click()));
    await choose(add, "Led by", "Mira");
    await act(async () => void Array.from(add.querySelectorAll("button")).find((b) => b.textContent === "Set")!.click());
    expect(api.setLeaderDefault).toHaveBeenCalledWith("project:shop", "persona:p1");
    expect(api.setStyleDefault).not.toHaveBeenCalled();
  });
});
