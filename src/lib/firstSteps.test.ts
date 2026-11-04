import { describe, expect, it } from "vitest";
import { canChatNow, needsFirstSteps, pickFirstModel } from "./firstSteps";
import type { CatalogEntry } from "./api";

const none = { libraryModels: [], providers: [], endpointModels: [] };

const entry = (id: string, fit: CatalogEntry["fit"]): CatalogEntry => ({
  id,
  name: id,
  description: "",
  quant: "Q4",
  size_mb: 2000,
  vision: false,
  url: `https://example.com/${id}.gguf`,
  source: "x",
  license: null,
  fit,
  speed: "",
});

describe("canChatNow", () => {
  it("is false with no model, no key and no server", () => {
    expect(canChatNow(none)).toBe(false);
  });
  it("is true with a chat model on this PC", () => {
    expect(canChatNow({ ...none, libraryModels: [{ role: "chat" }] })).toBe(true);
    expect(canChatNow({ ...none, libraryModels: [{}] })).toBe(true);
  });
  it("ignores models that are not for chat", () => {
    expect(canChatNow({ ...none, libraryModels: [{ role: "embed" }] })).toBe(false);
  });
  it("is true with a saved key, or a connected server", () => {
    expect(canChatNow({ ...none, providers: [{ key_set: false }, { key_set: true }] })).toBe(true);
    expect(canChatNow({ ...none, providers: [{ key_set: false }] })).toBe(false);
    expect(canChatNow({ ...none, endpointModels: [{}] })).toBe(true);
  });
});

describe("needsFirstSteps", () => {
  it("waits for the model lists, so a set-up user never sees a flash", () => {
    expect(needsFirstSteps({ ...none, modelsLoaded: false }, true)).toBe(false);
    expect(needsFirstSteps({ ...none, modelsLoaded: true }, true)).toBe(true);
  });
  it("stays out of the way once something can chat, and outside the desktop app", () => {
    expect(needsFirstSteps({ ...none, providers: [{ key_set: true }], modelsLoaded: true }, true)).toBe(false);
    expect(needsFirstSteps({ ...none, modelsLoaded: true }, false)).toBe(false);
  });
});

describe("pickFirstModel", () => {
  it("takes the first entry that fits this PC", () => {
    expect(pickFirstModel([entry("big", "wont-fit"), entry("ok", "slow"), entry("best", "great")])?.id).toBe("ok");
  });
  it("is null when nothing fits", () => {
    expect(pickFirstModel([entry("big", "wont-fit")])).toBeNull();
    expect(pickFirstModel([])).toBeNull();
  });
});
