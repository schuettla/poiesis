/**
 * @vitest-environment jsdom
 *
 * `TIER-T-6`: the two choices on a Models row. Features writes `models.tier.<id>`
 * and Auto clears it; the Memory window lists only what the model was trained
 * for and writes `models.ctx.<id>`.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const written: Array<[string, string]> = [];
const listModels = vi.fn();

vi.mock("../../lib/api", async (orig) => ({
  ...(await orig<typeof import("../../lib/api")>()),
  inTauri: () => true,
  setSetting: (k: string, v: string) => {
    written.push([k, v]);
    return Promise.resolve();
  },
  listModels: () => listModels(),
}));

import { TierControls } from "./TierControls";
import { useAppStore } from "../../lib/store";
import type { Model } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  written.length = 0;
  listModels.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const cloud: Model = {
  id: "cloud:openrouter:small/model",
  name: "Small",
  provenance: "cloud",
  provider: "openrouter",
  tier: "basic",
  autoTier: "basic",
  tierSet: null,
};

const cloudRow = {
  id: "openrouter:small/model",
  name: "Small",
  provider: "openrouter",
  model: "small/model",
  vision: false,
  tools: true,
  prompt_per_mtok: null,
  output_per_mtok: null,
  tier: "basic",
  auto_tier: "basic",
  tier_set: null,
};

const triggers = () => Array.from(container.querySelectorAll<HTMLButtonElement>(".select-trigger"));
const optionLabels = () => Array.from(container.querySelectorAll(".select-option-label")).map((e) => e.textContent);
async function pick(trigger: HTMLButtonElement, label: string) {
  await act(async () => trigger.click());
  const option = Array.from(container.querySelectorAll<HTMLElement>('[role="option"]')).find(
    (o) => o.querySelector(".select-option-label")?.textContent === label
  )!;
  await act(async () => option.click());
}

describe("Features", () => {
  it("shows what Auto gives, and offers the three tiers", async () => {
    useAppStore.setState({ models: [cloud], cloudModels: [], endpointModels: [] } as never);
    await act(async () => root.render(<TierControls model={cloud} />));
    expect(triggers()).toHaveLength(1);
    expect(triggers()[0].textContent).toBe("Auto · Basic");
    await act(async () => triggers()[0].click());
    expect(optionLabels()).toEqual(["Auto · Basic", "Full", "Basic", "Chat only"]);
  });

  it("writes the choice, and Auto clears it", async () => {
    useAppStore.setState({ models: [cloud], cloudModels: [cloudRow], endpointModels: [] } as never);
    await act(async () => root.render(<TierControls model={cloud} />));
    await pick(triggers()[0], "Full");
    expect(written).toEqual([["models.tier.cloud:openrouter:small/model", "full"]]);
    expect(useAppStore.getState().models[0]).toMatchObject({ tier: "full", tierSet: "full" });

    // The row now shows the choice; picking Auto puts back what the rule gives.
    const now = useAppStore.getState().models[0];
    await act(async () => root.render(<TierControls model={now} />));
    expect(triggers()[0].textContent).toBe("Full");
    await pick(triggers()[0], "Auto · Basic");
    expect(written[1]).toEqual(["models.tier.cloud:openrouter:small/model", ""]);
    expect(useAppStore.getState().models[0]).toMatchObject({ tier: "basic", tierSet: null });
  });

  it("has no Memory window for a model that is not on this PC", async () => {
    await act(async () => root.render(<TierControls model={cloud} />));
    expect(container.textContent).not.toContain("Memory window");
  });
});

describe("Memory window", () => {
  const local: Model = {
    id: "lib-1",
    name: "Tiny",
    provenance: "local",
    tier: "chat",
    autoTier: "chat",
    tierSet: null,
    window: 4096,
    autoWindow: 4096,
    windowSet: null,
    trainedCtx: 16384,
  };

  it("lists only the steps up to what the model was trained for", async () => {
    await act(async () => root.render(<TierControls model={local} />));
    expect(triggers()).toHaveLength(2);
    expect(triggers()[1].textContent).toBe("Auto · 4K");
    await act(async () => triggers()[1].click());
    expect(optionLabels()).toEqual(["Auto · 4K", "4K", "8K", "16K"]);
  });

  it("says a change waits for a reload, but not after a start that had to use less", async () => {
    const note = "Used next time the model loads";
    // Loaded meaning 4K, and Auto still 4K: nothing waiting.
    useAppStore.setState({ loadedModelId: "lib-1", loadedWindow: 4096 } as never);
    await act(async () => root.render(<TierControls model={local} />));
    expect(container.textContent).not.toContain(note);
    // The user picked 16K since: waiting.
    await act(async () => root.render(<TierControls model={{ ...local, window: 16384, windowSet: 16384 }} />));
    expect(container.textContent).toContain(note);
    // Not loaded: nothing to wait for.
    useAppStore.setState({ loadedModelId: null, loadedWindow: null } as never);
    await act(async () => root.render(<TierControls model={{ ...local, window: 16384, windowSet: 16384 }} />));
    expect(container.textContent).not.toContain(note);
  });

  it("writes the window and reads the library again, because the tier moves with it", async () => {
    listModels.mockResolvedValue([
      {
        id: "lib-1",
        name: "Tiny",
        path: "p",
        quant: null,
        size_bytes: 1,
        vision: false,
        role: "chat",
        is_default: true,
        added_at: 1,
        trained_ctx: 16384,
        window: 16384,
        auto_window: 4096,
        window_set: 16384,
        tier: "basic",
        auto_tier: "chat",
        tier_set: null,
      },
    ]);
    useAppStore.setState({
      models: [local],
      libraryModels: [],
      cloudModels: [],
      mediaModels: [],
      endpointModels: [],
    } as never);
    await act(async () => root.render(<TierControls model={local} />));
    await pick(triggers()[1], "16K");
    expect(written).toEqual([["models.ctx.lib-1", "16384"]]);
    expect(listModels).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().models.find((m) => m.id === "lib-1")).toMatchObject({
      window: 16384,
      tier: "basic",
    });
  });
});
