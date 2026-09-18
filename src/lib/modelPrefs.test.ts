import { describe, expect, it } from "vitest";
import type { Model } from "./types";
import type { ModelEntry } from "./api";
import {
  EMPTY_PREFS,
  moveFavorite,
  normalizePrefs,
  parseIdList,
  reorderFavorites,
  resolveChatModel,
  setDefault,
  skippedNotice,
  toggleFavorite,
  type ModelPrefs,
} from "./modelPrefs";

const local: Model = { id: "lib-1", name: "Qwen2.5 7B", provenance: "local" };
const server: Model = { id: "endpoint:e1:mistral", name: "mistral-nemo", provenance: "endpoint", endpointId: "e1" };
const claude: Model = {
  id: "cloud:anthropic:claude-sonnet-4-5",
  name: "Claude Sonnet 4.5",
  provenance: "cloud",
  provider: "anthropic",
};
const image: Model = { id: "media:openrouter/img", name: "Nano Banana", provenance: "cloud", modality: "image" };
const video: Model = { id: "media:openrouter/vid", name: "Veo", provenance: "cloud", modality: "video" };

const lib = (over: Partial<ModelEntry> = {}): ModelEntry => ({
  id: "lib-1",
  name: "Qwen2.5 7B",
  path: "C:/m/qwen.gguf",
  quant: "Q4_K_M",
  size_bytes: 1,
  vision: false,
  role: "chat",
  is_default: false,
  added_at: 0,
  ...over,
});

function prefs(over: Partial<ModelPrefs> = {}): ModelPrefs {
  return { ...EMPTY_PREFS, ...over };
}

describe("defaults across sources (MOD-3)", () => {
  it("picks the chat default even when it's a cloud model", () => {
    const p = setDefault(EMPTY_PREFS, claude);
    const r = resolveChatModel([local, claude], [lib({ is_default: true })], p);
    expect(r).toEqual({ id: claude.id, skippedDefault: null, via: "default" });
  });

  it("takes the next available favorite, in order, when the default is gone", () => {
    const p = prefs({
      defaults: { chat: claude.id, image: null },
      favorites: { chat: [claude.id, "cloud:openai:gone", server.id, local.id], media: [] },
      labels: { [claude.id]: "Claude Sonnet 4.5" },
    });
    const r = resolveChatModel([local, server], [], p);
    expect(r.id).toBe(server.id);
    expect(r.skippedDefault).toBe(claude.id);
    expect(skippedNotice(p, r, [local, server])).toBe(
      "Claude Sonnet 4.5 isn't available right now. Using mistral-nemo, your next favorite."
    );
  });

  it("falls back to today's order when nothing preferred is usable", () => {
    const r = resolveChatModel([image, local], [lib({ is_default: true })], EMPTY_PREFS);
    expect(r).toEqual({ id: local.id, skippedDefault: null, via: "fallback" });
  });

  it("never resolves to a media model", () => {
    const p = prefs({ defaults: { chat: image.id, image: null }, favorites: { chat: [image.id], media: [] } });
    expect(resolveChatModel([image, local], [], p).id).toBe(local.id);
  });

  it("has no video default", () => {
    expect(setDefault(EMPTY_PREFS, video)).toBe(EMPTY_PREFS);
    const p = setDefault(EMPTY_PREFS, image);
    expect(p.defaults.image).toBe(image.id);
    expect(p.defaults.chat).toBeNull();
  });
});

describe("favorites (MOD-4)", () => {
  it("making a model default moves it to slot 1", () => {
    let p = prefs({ favorites: { chat: [local.id, server.id], media: [] } });
    p = setDefault(p, server);
    expect(p.favorites.chat).toEqual([server.id, local.id]);
    p = setDefault(p, claude);
    expect(p.favorites.chat).toEqual([claude.id, server.id, local.id]);
  });

  it("new stars go to the end, per tab", () => {
    let p = toggleFavorite(EMPTY_PREFS, local)!;
    p = toggleFavorite(p, claude)!;
    p = toggleFavorite(p, video)!;
    expect(p.favorites.chat).toEqual([local.id, claude.id]);
    expect(p.favorites.media).toEqual([video.id]);
  });

  it("refuses to unstar the default", () => {
    const p = setDefault(EMPTY_PREFS, local);
    expect(toggleFavorite(p, local)).toBeNull();
    expect(toggleFavorite(toggleFavorite(p, claude)!, claude)!.favorites.chat).toEqual([local.id]);
  });

  it("keyboard moves keep the default in slot 1", () => {
    let p = setDefault(prefs({ favorites: { chat: [server.id, claude.id], media: [] } }), local);
    expect(p.favorites.chat).toEqual([local.id, server.id, claude.id]);
    expect(moveFavorite(p, "chat", server.id, -1)).toBe(p);
    expect(moveFavorite(p, "chat", local.id, 1)).toBe(p);
    p = moveFavorite(p, "chat", claude.id, -1);
    expect(p.favorites.chat).toEqual([local.id, claude.id, server.id]);
  });

  it("a dragged order persists, with the default pinned first", () => {
    const p = setDefault(prefs({ favorites: { chat: [server.id, claude.id], media: [] } }), local);
    const next = reorderFavorites(p, "chat", [claude.id, local.id, server.id]);
    expect(next.favorites.chat).toEqual([local.id, claude.id, server.id]);
  });

  it("seeds the default from the library once, and favorites the first model", () => {
    const seeded = normalizePrefs(EMPTY_PREFS, [lib({ is_default: true })], [local]);
    expect(seeded.defaults.chat).toBe(local.id);
    expect(seeded.favorites.chat).toEqual([local.id]);
    const fresh = normalizePrefs(EMPTY_PREFS, [], [claude]);
    expect(fresh.favorites.chat).toEqual([claude.id]);
    expect(fresh.defaults.chat).toBeNull();
  });

  it("reads a stored list defensively", () => {
    expect(parseIdList('["a","b","a",3]')).toEqual(["a", "b"]);
    expect(parseIdList("not json")).toEqual([]);
    expect(parseIdList(null)).toEqual([]);
  });
});
