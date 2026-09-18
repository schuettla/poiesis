/**
 * @vitest-environment jsdom
 *
 * `MOD-4` in the picker: favorites lead in the user's order, the cloud limit
 * never cuts them, and the empty-cloud links go where keys and servers live
 * now (`PRV-6`). Plus `MOD-3`'s startup settle, which decides what the picker
 * shows selected.
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

const local: Model = { id: "lib-1", name: "Qwen2.5 7B", provenance: "local", available: true };
const cloud = (i: number): Model => ({
  id: `cloud:openrouter:m${i}`,
  name: `Cloud ${String(i).padStart(3, "0")}`,
  provenance: "cloud",
  provider: "openrouter",
  available: true,
});

async function openPicker() {
  await act(async () => root.render(<ModelPicker />));
  await act(async () => container.querySelector<HTMLButtonElement>(".model-picker-trigger")!.click());
}

const groupLabels = () =>
  Array.from(container.querySelectorAll(".model-group-label")).map((el) => el.textContent);
const rowNames = () => Array.from(container.querySelectorAll(".model-option .name")).map((el) => el.textContent);

describe("the model picker with favorites", () => {
  it("leads with favorites in order, and the cloud limit never cuts them", async () => {
    const many = Array.from({ length: 80 }, (_, i) => cloud(i));
    useAppStore.setState({
      models: [local, ...many],
      selectedModelId: local.id,
      modelFilter: "all",
      modelPrefs: {
        ...EMPTY_PREFS,
        // #75 is far past `CLOUD_LIMIT` (60) in the cloud group's own order.
        favorites: { chat: [many[75].id, local.id], media: [] },
      },
    } as never);
    await openPicker();
    expect(groupLabels()[0]).toBe("Favorites");
    expect(rowNames().slice(0, 2)).toEqual(["Cloud 075", "Qwen2.5 7B"]);
    // Shown once: not repeated in its own group below.
    expect(rowNames().filter((n) => n === "Cloud 075")).toHaveLength(1);
  });

  it("sends the empty-cloud links to Providers and to Runtime → Your servers", async () => {
    useAppStore.setState({
      models: [local],
      selectedModelId: local.id,
      modelFilter: "all",
      modelPrefs: EMPTY_PREFS,
      view: "chat",
    } as never);
    await openPicker();
    const links = Array.from(container.querySelectorAll<HTMLAnchorElement>(".add-key-row a"));
    await act(async () => links[0].click());
    expect(useAppStore.getState().view).toBe("providers");
    await openPicker();
    const again = Array.from(container.querySelectorAll<HTMLAnchorElement>(".add-key-row a"));
    await act(async () => again[1].click());
    expect(useAppStore.getState().view).toBe("runtime");
    expect(useAppStore.getState().runtimeTab).toBe("servers");
  });
});

describe("a new chat (MOD-3)", () => {
  it("opens on the default, even after another model was used", async () => {
    const nemotron = cloud(3);
    useAppStore.setState({
      models: [local, nemotron],
      libraryModels: [],
      conversations: [],
      selectedModelId: local.id,
      selectionSettled: true,
      modelNotice: null,
      modelNoticeFor: null,
      modelPrefs: {
        ...EMPTY_PREFS,
        defaults: { chat: nemotron.id, image: null },
        favorites: { chat: [nemotron.id, local.id], media: [] },
      },
    } as never);
    await act(async () => useAppStore.getState().newConversation());
    expect(useAppStore.getState().selectedModelId).toBe(nemotron.id);
    expect(useAppStore.getState().modelNotice).toBeNull();
  });
});

describe("startup selection (MOD-3)", () => {
  it("follows a cloud default, and says once when it has to skip it", () => {
    const claude = cloud(1);
    const server: Model = { id: "endpoint:e1:x", name: "mistral-nemo", provenance: "endpoint", available: true };
    // The default's list has arrived: it's used, with no notice.
    useAppStore.setState({
      models: [local, claude],
      libraryModels: [],
      selectedModelId: local.id,
      selectionSettled: false,
      modelPrefsLoaded: true,
      modelNotice: null,
      modelNoticeFor: null,
      modelPrefs: {
        ...EMPTY_PREFS,
        defaults: { chat: claude.id, image: null },
        favorites: { chat: [claude.id, server.id], media: [] },
        labels: { [claude.id]: "Cloud 001" },
      },
    } as never);
    useAppStore.getState().settleSelection();
    expect(useAppStore.getState().selectedModelId).toBe(claude.id);
    expect(useAppStore.getState().modelNotice).toBeNull();

    // Its key was removed: the next available favorite, and one notice.
    useAppStore.setState({
      models: [local, server],
      selectedModelId: local.id,
      selectionSettled: false,
    } as never);
    useAppStore.getState().settleSelection();
    expect(useAppStore.getState().selectedModelId).toBe(server.id);
    expect(useAppStore.getState().modelNotice).toBe(
      "Cloud 001 isn't available right now. Using mistral-nemo, your next favorite."
    );
  });
});
