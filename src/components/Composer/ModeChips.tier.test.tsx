/**
 * @vitest-environment jsdom
 *
 * `TIER-T-6`: the composer chip that says a small model has fewer features. It
 * shows only when the model's tier takes away something the user has on, its
 * label opens the Models page at that model, its x gives the features back, and
 * the chip that follows takes that back again.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const written: Array<[string, string]> = [];
vi.mock("../../lib/api", async (orig) => ({
  ...(await orig<typeof import("../../lib/api")>()),
  inTauri: () => true,
  setSetting: (k: string, v: string) => {
    written.push([k, v]);
    return Promise.resolve();
  },
}));

import ModeChips from "./ModeChips";
import { useAppStore } from "../../lib/store";
import type { Model } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  written.length = 0;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const model = (over: Partial<Model> = {}): Model => ({
  id: "cloud:openrouter:small",
  name: "Small",
  provenance: "cloud",
  provider: "openrouter",
  tier: "basic",
  autoTier: "basic",
  tierSet: null,
  ...over,
});

const cloudRow = {
  id: "openrouter:small",
  name: "Small",
  provider: "openrouter",
  model: "small",
  vision: false,
  tools: true,
  prompt_per_mtok: null,
  output_per_mtok: null,
  tier: "basic",
  auto_tier: "basic",
  tier_set: null,
};

async function show(m: Model, toolsEnabled = true, activeRun: unknown = null) {
  useAppStore.setState({
    models: [m],
    selectedModelId: m.id,
    toolsEnabled,
    workspaceMode: false,
    cloudModels: [{ ...cloudRow, tier: m.tier, auto_tier: m.autoTier, tier_set: m.tierSet }],
    endpointModels: [],
    libraryModels: [],
    mediaModels: [],
    view: "chat",
    focusedModelId: null,
    activeConversationId: "c1",
    activeRun,
  } as never);
  await act(async () => root.render(<ModeChips />));
}
const labels = () => Array.from(container.querySelectorAll(".mode-chip-label")).map((e) => e.textContent);
const title = () => container.querySelector<HTMLElement>(".mode-chip-label")?.title;

describe("the tier chip", () => {
  it("names Basic and Chat in a word, like the other chips, with the reason on hover", async () => {
    await show(model({ tier: "basic" }));
    expect(labels()).toEqual(["Basic tools"]);
    expect(title()).toBe("Small memory, so I use only files, web search and memory. · Change this");
    await show(model({ tier: "chat", autoTier: "chat" }));
    expect(labels()).toEqual(["Chat only"]);
    await show(model({ tier: "chat", autoTier: "chat", tools: false }));
    expect(title()).toContain("This model can't use tools.");
    await show(model({ tier: "chat", tierSet: "chat" }));
    expect(title()).toBe("You set this model to chat only. · Change this");
  });

  it("is not there at Full, or when the user already turned tools off", async () => {
    await show(model({ tier: "full", autoTier: "full" }));
    expect(labels()).toEqual([]);
    await show(model({ tier: "basic" }), false);
    expect(labels()).toEqual(["Tools off"]);
  });

  it("says the tier a live run really has", async () => {
    await show(model({ tier: "full", autoTier: "full" }), true, { convId: "c1", tier: "chat" });
    expect(labels()).toEqual(["Chat only"]);
  });

  it("opens the Models page at that model", async () => {
    await show(model());
    await act(async () => container.querySelector<HTMLButtonElement>(".mode-chip-label")!.click());
    expect(useAppStore.getState().view).toBe("models");
    expect(useAppStore.getState().focusedModelId).toBe("cloud:openrouter:small");
  });

  it("gives the features back with the x, and the next chip takes that back", async () => {
    await show(model());
    const x = () => container.querySelector<HTMLButtonElement>(".mode-chip-x")!;
    expect(x().getAttribute("aria-label")).toBe("Use all features with this model");
    await act(async () => x().click());
    expect(written).toEqual([["models.tier.cloud:openrouter:small", "full"]]);
    expect(labels()).toEqual(["All features"]);

    expect(x().getAttribute("aria-label")).toBe("Go back to fewer features for this small model");
    await act(async () => x().click());
    expect(written[1]).toEqual(["models.tier.cloud:openrouter:small", ""]);
    expect(labels()).toEqual(["Basic tools"]);
  });

  it("does not offer to go back for a model that is Full on its own", async () => {
    await show(model({ tier: "full", autoTier: "full", tierSet: "full" }));
    expect(labels()).toEqual([]);
  });
});
