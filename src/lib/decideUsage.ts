import type { DecisionUsageRow } from "./api";

/** `CTL-UI-2`: what the Usage screen's "Decisions" section says. */

export interface DecisionTotals {
  total: number;
  local: number;
  cloud: number;
  /** USD the cloud charged, for the answers whose reply said. */
  cloudCost: number;
}

export function decisionTotals(rows: DecisionUsageRow[]): DecisionTotals {
  const sum = (backend: DecisionUsageRow["backend"]) =>
    rows.filter((r) => r.backend === backend).reduce((n, r) => n + r.count, 0);
  const local = sum("local");
  const cloud = sum("openrouter");
  return {
    total: local + cloud,
    local,
    cloud,
    cloudCost: rows.filter((r) => r.backend === "openrouter").reduce((n, r) => n + r.cost_usd, 0),
  };
}

/** A price in dollars, as small as these are: a thousand decisions cost cents. */
export function smallMoney(usd: number): string {
  if (usd <= 0) return "$0.00";
  if (usd < 0.01) return "under $0.01";
  return `$${usd.toFixed(2)}`;
}

/** The right-hand column of a row. Local is free and says where it ran; the cloud
 * says what it charged, or that it did not say. */
export function decisionCostText(row: Pick<DecisionUsageRow, "backend" | "cost_usd">): string {
  if (row.backend === "local") return "free · on this computer";
  if (row.cost_usd <= 0) return "cost not reported";
  return smallMoney(row.cost_usd);
}

/** The line above the rows. */
export function decisionSummary(t: DecisionTotals): string {
  const word = t.total === 1 ? "quick decision" : "quick decisions";
  if (t.cloud === 0) return `${t.total} ${word}, all on this computer.`;
  const cost = t.cloudCost > 0 ? ` The cloud charged ${smallMoney(t.cloudCost)}.` : "";
  return `${t.total} ${word}: ${t.local} on this computer, ${t.cloud} in the cloud.${cost}`;
}

/** The count of one row, as a phrase. */
export function decisionCountText(row: Pick<DecisionUsageRow, "count" | "avg_ms">): string {
  const n = `${row.count} ${row.count === 1 ? "decision" : "decisions"}`;
  return row.avg_ms > 0 ? `${n} · ${Math.round(row.avg_ms)} ms` : n;
}
