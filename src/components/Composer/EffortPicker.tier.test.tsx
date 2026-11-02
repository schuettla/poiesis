/**
 * @vitest-environment jsdom
 *
 * `TIER-UI-4`: below Full the run asks for no thinking, so the picker says Off
 * with an outline meter and says why. It is not disabled: picking a level sets
 * the model's Features to Full, which is what makes the pick take effect.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const saved: Array<[string, string]> = [];
vi.mock("../../lib/api", () => ({
  inTauri: () => true,
  getSetting: () => Promise.resolve("high"),
  setSetting: (k: string, v: string) => {
    saved.push([k, v]);
    return Promise.resolve();
  },
}));

import EffortPicker, { LIMITED_HINT } from "./EffortPicker";
import { useAppStore } from "../../lib/store";
import type { Model } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  saved.length = 0;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const small: Model = {
  id: "cloud:openrouter:small",
  name: "Small",
  provenance: "cloud",
  provider: "openrouter",
  tier: "basic",
  autoTier: "basic",
  tierSet: null,
};
const trigger = () => container.querySelector<HTMLButtonElement>(".effort-trigger")!;

async function show(m: Model) {
  useAppStore.setState({ models: [m], selectedModelId: m.id, cloudModels: [], endpointModels: [] } as never);
  await act(async () => root.render(<EffortPicker />));
}

describe("the effort picker below Full", () => {
  it("says Off with the outline meter and the reason, and is not disabled", async () => {
    await show(small);
    expect(trigger().textContent).toContain("Off");
    expect(trigger().title).toBe(LIMITED_HINT);
    expect(trigger().disabled).toBe(false);
    expect(container.querySelectorAll(".effort-meter i.unknown")).toHaveLength(3);
  });

  it("shows the stored level again at Full", async () => {
    await show({ ...small, tier: "full" });
    expect(trigger().textContent).toContain("High");
    expect(container.querySelectorAll(".effort-meter i.unknown")).toHaveLength(0);
  });

  it("makes a pick take effect by setting Features to Full", async () => {
    await show(small);
    await act(async () => trigger().click());
    const medium = Array.from(container.querySelectorAll<HTMLElement>(".effort-option")).find(
      (o) => o.querySelector(".name")?.textContent === "Medium"
    )!;
    await act(async () => medium.click());
    expect(saved).toContainEqual(["models.reasoning_effort", "medium"]);
    expect(saved).toContainEqual(["models.tier.cloud:openrouter:small", "full"]);
  });
});
