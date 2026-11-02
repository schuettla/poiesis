import { describe, expect, it } from "vitest";
import { tierOf, tierReason, tierTag, tokensLabel, windowSteps } from "./tier";
import type { Model } from "./types";

const base: Model = { id: "m", name: "M", provenance: "cloud" };

describe("the tier words (TIER-UI-1)", () => {
  it("shows nothing at Full and a tag below it", () => {
    expect(tierTag({ ...base, tier: "full" })).toBeNull();
    expect(tierTag({ ...base })).toBeNull();
    expect(tierTag({ ...base, tier: "basic" })?.label).toBe("basic tools");
    expect(tierTag({ ...base, tier: "chat" })?.label).toBe("chat only");
    expect(tierTag({ ...base, tier: "chat" })?.chip).toBe("Chat only");
    expect(tierTag({ ...base, tier: "basic" })?.chip).toBe("Basic tools");
  });

  it("says why, in one line each", () => {
    expect(tierReason({ ...base, tier: "chat", tools: false })).toBe(
      "This model can't use tools. It can chat, but it can't search, read files, or run skills."
    );
    expect(tierReason({ ...base, tier: "chat", tools: true })).toBe("Small memory, so tools are off and answers fit.");
    expect(tierReason({ ...base, tier: "basic" })).toBe("Small memory, so I use only files, web search and memory.");
    expect(tierReason({ ...base, tier: "full" })).toBeNull();
  });

  it("does not blame the memory for a choice the user made", () => {
    expect(tierReason({ ...base, tier: "chat", tierSet: "chat" })).toBe("You set this model to chat only.");
    expect(tierReason({ ...base, tier: "basic", tierSet: "basic" })).toBe("You set this model to basic tools.");
  });

  it("keeps what tools:false always meant when no tier arrived", () => {
    expect(tierOf({ ...base, tools: false })).toBe("chat");
    expect(tierOf({ ...base, tools: false, tier: "full" })).toBe("full");
    expect(tierOf(base)).toBe("full");
  });
});

describe("the memory window steps (TIER-UI-3)", () => {
  it("lists only steps up to what the model was trained for", () => {
    expect(windowSteps({ ...base, trainedCtx: 16384 })).toEqual([4096, 8192, 16384]);
    expect(windowSteps({ ...base, trainedCtx: 131072 })).toEqual([4096, 8192, 16384, 32768, 65536, 131072]);
  });

  it("stops at the cap Auto never passes when the file could not be read", () => {
    expect(windowSteps(base)).toEqual([4096, 8192, 16384, 32768]);
  });

  it("keeps a value that was set before, so the select can show it", () => {
    expect(windowSteps({ ...base, trainedCtx: 8192, windowSet: 6144 })).toEqual([4096, 6144, 8192]);
  });

  it("words a size as K", () => {
    expect(tokensLabel(4096)).toBe("4K");
    expect(tokensLabel(131072)).toBe("128K");
    expect(tokensLabel(512)).toBe("512");
  });
});
