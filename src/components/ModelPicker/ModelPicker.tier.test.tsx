/**
 * @vitest-environment jsdom
 *
 * `TIER-UI-1`: the picker tells a model that can carry less, with the reason on
 * hover. Full says nothing.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import ModelPicker from "./ModelPicker";
import { useAppStore } from "../../lib/store";
import { EMPTY_PREFS } from "../../lib/modelPrefs";
import type { Model } from "../../lib/types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const m = (id: string, over: Partial<Model>): Model => ({
  id,
  name: id,
  provenance: "cloud",
  provider: "openrouter",
  available: true,
  ...over,
});

describe("the tier tag in the picker", () => {
  it("tags Chat and Basic with the reason, and leaves Full alone", async () => {
    const models = [
      m("big", { tier: "full" }),
      m("notools", { tier: "chat", tools: false }),
      m("tiny", { tier: "chat", tools: true }),
      m("mid", { tier: "basic" }),
    ];
    useAppStore.setState({
      models,
      selectedModelId: "big",
      modelFilter: "all",
      modelPrefs: EMPTY_PREFS,
    } as never);
    await act(async () => root.render(<ModelPicker />));
    await act(async () => container.querySelector<HTMLButtonElement>(".model-picker-trigger")!.click());

    const tagOf = (name: string) =>
      Array.from(container.querySelectorAll(".model-option"))
        .find((row) => row.querySelector(".name")?.textContent === name)
        ?.querySelector<HTMLElement>(".no-tools") ?? null;

    expect(tagOf("big")).toBeNull();
    expect(tagOf("notools")?.textContent).toBe("chat only");
    expect(tagOf("notools")?.title).toBe(
      "This model can't use tools. It can chat, but it can't search, read files, or run skills."
    );
    expect(tagOf("tiny")?.textContent).toBe("chat only");
    expect(tagOf("tiny")?.title).toBe("Small memory, so tools are off and answers fit.");
    expect(tagOf("mid")?.textContent).toBe("basic tools");
    expect(tagOf("mid")?.title).toBe("Small memory, so I use only files, web search and memory.");
  });
});
