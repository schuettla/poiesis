import { describe, expect, it } from "vitest";
import { applyTending, useAppStore, type AppState } from "./store";

const state = (over: Partial<AppState>) => ({ ...useAppStore.getState(), ...over }) as AppState;

describe("the tidy-up and the mark (SLP-1)", () => {
  it("is tending while the pass runs, unless a turn is streaming", () => {
    expect(applyTending(state({ busyChats: {}, presence: "idle" }), true)).toEqual({ presence: "tending" });
    expect(applyTending(state({ busyChats: { c1: "other" }, presence: "active" }), true)).toEqual({});
  });

  it("falls back to whatever is underneath when it ends", () => {
    expect(applyTending(state({ busyChats: {}, presence: "tending", reflectingIds: [] }), false)).toEqual({ presence: "idle" });
    expect(applyTending(state({ busyChats: {}, presence: "tending", reflectingIds: ["c1"] }), false)).toEqual({
      presence: "reflecting",
    });
    expect(applyTending(state({ busyChats: { c1: "other" }, presence: "tending" }), false)).toEqual({ presence: "active" });
  });

  it("leaves a mark that is doing something else alone", () => {
    expect(applyTending(state({ presence: "healing" }), false)).toEqual({});
  });
});
