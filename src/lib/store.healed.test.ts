import { describe, expect, it } from "vitest";
import { applyHealed } from "./store";

describe("the watchdog's report (HEAL-1)", () => {
  it("leaves a restarted runtime ready", () => {
    expect(applyHealed({ attempt: 1, ok: true })).not.toHaveProperty("engineReady");
  });

  it("stops calling a runtime ready once repair has failed", () => {
    expect(applyHealed({ attempt: 3, ok: false })).toMatchObject({
      engineReady: false,
      engineFailed: true,
      loadedModelId: null,
    });
  });
});
