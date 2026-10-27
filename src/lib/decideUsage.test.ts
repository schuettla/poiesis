import { describe, expect, it } from "vitest";
import type { DecisionUsageRow } from "./api";
import { decisionCostText, decisionCountText, decisionSummary, decisionTotals, smallMoney } from "./decideUsage";

const row = (over: Partial<DecisionUsageRow> = {}): DecisionUsageRow => ({
  backend: "local",
  model: "Clef-Flash-Q4_K_M",
  count: 10,
  cost_usd: 0,
  input_tokens: 1000,
  avg_ms: 40,
  ...over,
});
const cloud = (over: Partial<DecisionUsageRow> = {}) =>
  row({ backend: "openrouter", model: "cloudflare/clef-flash", count: 4, cost_usd: 0.000_084, avg_ms: 210, ...over });

describe("decisionTotals (CTL-UI-2)", () => {
  it("splits the count by where it was asked and adds up what the cloud charged", () => {
    const t = decisionTotals([row(), row({ model: "Clef-Q4_K_M", count: 5 }), cloud(), cloud({ model: "typesafe/jev", cost_usd: 0.000_016 })]);
    expect([t.total, t.local, t.cloud]).toEqual([23, 15, 8]);
    expect(t.cloudCost).toBeCloseTo(0.0001, 10);
    expect(decisionTotals([])).toEqual({ total: 0, local: 0, cloud: 0, cloudCost: 0 });
  });
});

describe("what the section says", () => {
  it("calls local free and says where it ran", () => {
    expect(decisionCostText(row())).toBe("free · on this computer");
  });

  it("gives the cloud's charge, in small amounts, or says it did not report one", () => {
    expect(decisionCostText(cloud())).toBe("under $0.01");
    expect(decisionCostText(cloud({ cost_usd: 1.234 }))).toBe("$1.23");
    expect(decisionCostText(cloud({ cost_usd: 0 }))).toBe("cost not reported");
    expect(smallMoney(0)).toBe("$0.00");
  });

  it("sums up all-local and mixed use", () => {
    expect(decisionSummary(decisionTotals([row({ count: 1 })]))).toBe("1 quick decision, all on this computer.");
    expect(decisionSummary(decisionTotals([row(), cloud()]))).toBe(
      "14 quick decisions: 10 on this computer, 4 in the cloud. The cloud charged under $0.01."
    );
    expect(decisionSummary(decisionTotals([row(), cloud({ cost_usd: 0 })]))).toBe(
      "14 quick decisions: 10 on this computer, 4 in the cloud."
    );
  });

  it("writes a row's count with its average time", () => {
    expect(decisionCountText(row({ count: 1, avg_ms: 0 }))).toBe("1 decision");
    expect(decisionCountText(row({ count: 12, avg_ms: 38.4 }))).toBe("12 decisions · 38 ms");
  });
});
