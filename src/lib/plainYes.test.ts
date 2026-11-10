import { describe, expect, it } from "vitest";
import { isPlainYes } from "./store";

// `PLF-4`: typed under a plan that waits, a plain yes is the Go ahead.
describe("isPlainYes", () => {
  it("hears a short yes", () => {
    for (const t of ["go on", "Go ahead!", "yes", "ok", "do it", "yes, go ahead please", "ja, mach", "weiter", "LGTM"]) {
      expect(isPlainYes(t), t).toBe(true);
    }
  });
  it("leaves anything else to be a change", () => {
    for (const t of ["", "go on but skip the vendor part", "no", "make it shorter", "yes but in German", "ok so what about risks"]) {
      expect(isPlainYes(t), t).toBe(false);
    }
  });
});
