/**
 * @vitest-environment jsdom
 *
 * `TEAM-5`: Self → Memory can show whose entries it lists: everyone's, what Poiesis (everyone)
 * knows, or what one persona alone knows. A link from a persona's page lands on its own.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Fact } from "../../lib/api";

const entry = (name: string, persona: string | null): Fact =>
  ({ name, description: name, kind: "preference", created: "2026-11-01", source_conversation: null, body: `body of ${name}`, scope: "global", persona }) as Fact;

const api = vi.hoisted(() => ({ shareMemory: vi.fn() }));
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  listMemoryFacts: () =>
    Promise.resolve([entry("company-name", null), entry("short-intros", "p1"), entry("long-intros", "p2")]),
  listEarlierVersions: () => Promise.resolve([]),
  listUnsure: () => Promise.resolve([]),
  listLetGo: () => Promise.resolve([]),
  listRecentChanges: () => Promise.resolve([]),
  getProfile: () => Promise.resolve(null),
  getPendingConsolidation: () => Promise.resolve(null),
  ...api,
}));

import { useAppStore } from "../../lib/store";
import MemoryPanel from "./MemoryPanel";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const flush = () => act(async () => void (await Promise.resolve()));
const names = () => Array.from(container.querySelectorAll(".memory-card .memory-name")).map((n) => n.textContent);

async function mount() {
  act(() => root.render(<MemoryPanel />));
  await flush();
  await flush();
}

async function pickWhose(label: string) {
  act(() => container.querySelector<HTMLButtonElement>(".memory-whose button")!.click());
  const option = Array.from(container.querySelectorAll<HTMLElement>("[role=option]")).find((o) => o.textContent?.startsWith(label))!;
  await act(async () => option.click());
}

beforeEach(() => {
  api.shareMemory.mockReset().mockResolvedValue(undefined);
  useAppStore.setState({
    refreshMemoryContext: () => Promise.resolve(),
    refreshChangeProposals: () => Promise.resolve(),
    conversations: [],
    personas: [
      { id: "p1", name: "Mira" },
      { id: "p2", name: "Atlas" },
    ],
    selfTabRequest: null,
  } as never);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("whose entries", () => {
  it("shows everything by default, and says on a tagged entry whose it is", async () => {
    await mount();
    expect(names()).toEqual(["company-name", "short-intros", "long-intros"]);
    const tags = Array.from(container.querySelectorAll(".memory-card-head .memory-kind")).map((t) => t.textContent);
    expect(tags).toContain("Mira only");
    expect(tags).toContain("Atlas only");
  });

  it("shows only what everyone knows for Poiesis, and only the tagged ones for a persona", async () => {
    await mount();
    await pickWhose("Poiesis");
    expect(names()).toEqual(["company-name"]);
    await pickWhose("Mira");
    expect(names()).toEqual(["short-intros"]);
    await pickWhose("Everyone");
    expect(names()).toHaveLength(3);
  });

  it("is not there at all until there is a persona", async () => {
    useAppStore.setState({ personas: [] } as never);
    await mount();
    expect(container.querySelector(".memory-whose")).toBeNull();
  });

  it("lands on a persona when a link from her page asks", async () => {
    useAppStore.setState({ selfTabRequest: { tab: "memory", nonce: 1, focus: "persona:p1" } } as never);
    await mount();
    expect(names()).toEqual(["short-intros"]);
  });

  it("lands on Poiesis when its page asks", async () => {
    useAppStore.setState({ selfTabRequest: { tab: "memory", nonce: 2, focus: "poiesis" } } as never);
    await mount();
    expect(names()).toEqual(["company-name"]);
  });

  it("shares a tagged entry with everyone from its card", async () => {
    await mount();
    const card = Array.from(container.querySelectorAll(".memory-card")).find((c) => c.textContent?.includes("short-intros"))!;
    await act(async () => void Array.from(card.querySelectorAll("button")).find((b) => b.textContent === "Share with everyone")!.click());
    expect(api.shareMemory).toHaveBeenCalledWith("facts", "short-intros");
    const shared = Array.from(container.querySelectorAll(".memory-card")).find((c) => c.textContent?.includes("company-name"))!;
    expect(Array.from(shared.querySelectorAll("button")).some((b) => b.textContent === "Share with everyone")).toBe(false);
  });
});
