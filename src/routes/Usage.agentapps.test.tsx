/**
 * @vitest-environment jsdom
 *
 * Settings → Usage → Agent apps (`CLI-UI-7`): the jobs agent apps did are listed
 * on their own, with who paid and a grey list-price figure where the app reports
 * one, and they never appear in the cloud totals.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PricedUsage, UsageBucket } from "../lib/api";

const usageSummary = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  inTauri: () => true,
  usageSummary: (...a: unknown[]) => usageSummary(...a),
  composioUsage: () => Promise.resolve([]),
}));

import Usage from "./Usage";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollIntoView = () => {};

const bucket = (over: Partial<UsageBucket> = {}): UsageBucket => ({
  key: "x",
  label: null,
  provenance: "cloud",
  prompt_tokens: 0,
  output_tokens: 0,
  runs: 0,
  cost_usd: null,
  ...over,
});

const usage = (over: Partial<PricedUsage> = {}): PricedUsage => ({
  total: bucket(),
  by_day: [],
  by_model: [],
  by_conversation: [],
  some_prices_unknown: false,
  agent_apps: [],
  ...over,
});

let container: HTMLDivElement;
let root: Root;
const text = () => container.textContent ?? "";
const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));

async function show(data: PricedUsage) {
  usageSummary.mockResolvedValue(data);
  act(() => root.render(<Usage />));
  await settle();
}

beforeEach(() => {
  usageSummary.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("Usage: agent apps", () => {
  it("lists each app with its jobs, time, tokens and who paid, and says it is outside the totals", async () => {
    await show(
      usage({
        agent_apps: [
          { name: "Claude Code", runs: 2, prompt_tokens: 8000, output_tokens: 600, duration_ms: 125_000, notional_usd: 0.35, paid_by: "plan" },
          { name: "Codex", runs: 1, prompt_tokens: 9000, output_tokens: 700, duration_ms: 30_000, notional_usd: null, paid_by: "plan" },
        ],
      }),
    );
    const section = container.querySelector('section[aria-label="Agent apps"]')!;
    expect(section).not.toBeNull();
    const rows = [...section.querySelectorAll(".usage-app")].map((r) => r.textContent);
    expect(rows[0]).toContain("Claude Code");
    expect(rows[0]).toContain("2 jobs · 2 min 5 s");
    expect(rows[0]).toContain("8.0k in · 600 out");
    expect(rows[0]).toContain("on your plan");
    expect(rows[0]).toContain("≈ $0.35 at list prices");
    expect(rows[1]).toContain("1 job · 30 s");
    expect(rows[1]).not.toContain("list prices");
    expect(section.textContent).toContain("not in the totals above");
  });

  it("grays the list price and explains it in words, per who paid", async () => {
    await show(
      usage({
        agent_apps: [
          { name: "Claude Code", runs: 1, prompt_tokens: 1, output_tokens: 1, duration_ms: 1000, notional_usd: 0.1, paid_by: "plan" },
          { name: "Codex", runs: 1, prompt_tokens: 1, output_tokens: 1, duration_ms: 1000, notional_usd: 0.1, paid_by: "api-key" },
          { name: "OpenCode", runs: 1, prompt_tokens: 1, output_tokens: 1, duration_ms: 1000, notional_usd: null, paid_by: "free" },
        ],
      }),
    );
    const tips = [...container.querySelectorAll(".usage-app .usage-cost")].map((c) => c.getAttribute("title"));
    expect(tips[0]).toBe("Reported by Claude Code. On your plan this isn't charged per job.");
    expect(tips[1]).toContain("billed to the API key you gave it");
    expect(tips[2]).toBe("OpenCode doesn't report a price.");
    expect(container.querySelector(".usage-notional")).not.toBeNull();
  });

  it("shows the group even when the only jobs in the range were agent apps' ", async () => {
    await show(
      usage({
        agent_apps: [{ name: "Codex", runs: 1, prompt_tokens: 10, output_tokens: 5, duration_ms: 2000, notional_usd: null, paid_by: "plan" }],
      }),
    );
    expect(text()).not.toContain("Nothing to show for this range");
    expect(text()).toContain("Agent apps");
  });

  it("shows no group when no agent app did anything", async () => {
    await show(usage({ total: bucket({ runs: 1, prompt_tokens: 10, output_tokens: 5 }), by_day: [bucket({ key: "0", runs: 1 })] }));
    expect(container.querySelector('section[aria-label="Agent apps"]')).toBeNull();
  });

  it("keeps working when the backend sends no agent_apps at all", async () => {
    const old = usage({ total: bucket({ runs: 1, prompt_tokens: 10, output_tokens: 5 }) });
    delete old.agent_apps;
    await show(old);
    expect(container.querySelector('section[aria-label="Agent apps"]')).toBeNull();
  });
});
