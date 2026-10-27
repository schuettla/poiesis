import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "../lib/api";
import { inTauri } from "../lib/api";
import { decisionCostText, decisionCountText, decisionSummary, decisionTotals } from "../lib/decideUsage";
import { paidByLabel } from "../lib/engines";
import { useAppStore } from "../lib/store";
import "./Usage.css";

/**
 * `OBS-2`/`HRN-UI-6`: what runs have cost, by day, by model, by conversation.
 *
 * The honesty rule runs through the whole panel: a token count is a fact, a
 * price is a lookup that can miss, and a run on this machine costs nothing at
 * all. Those three are never blurred into one number — an unpriced model shows
 * its tokens and says so, and the total says it is a floor when anything in it
 * could not be priced.
 */

const RANGES = [
  { days: 1, label: "Today" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
] as const;

function tokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

function money(usd: number): string {
  return usd < 0.01 ? "under $0.01" : `$${usd.toFixed(2)}`;
}

/** What the right-hand column of a row says. Local runs are the interesting
 * case: they have real tokens and no cost, and saying "$0.00" would read as a
 * measurement rather than as the point. */
function priceLine(row: api.UsageBucket): string {
  if (row.provenance === "local") return "on your device, no cost";
  if (row.cost_usd !== null) return money(row.cost_usd);
  return "price unknown";
}

/** Not every provider reports what a turn used — several free tiers report
 * nothing at all. The run still happened and is still counted; saying
 * "0 in · 0 out" would claim a measurement we never got. */
function tokenLine(row: { prompt_tokens: number; output_tokens: number }): string {
  if (row.prompt_tokens === 0 && row.output_tokens === 0)
    return "tokens not reported";
  return `${tokens(row.prompt_tokens)} in · ${tokens(row.output_tokens)} out`;
}

function duration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min ${s % 60} s` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** An agent app's money column: who paid, then what it says the jobs would have
 * cost at list prices. That figure is grey on purpose: on a plan nothing is
 * charged per job, and it is in none of the totals. */
function appCost(row: api.AgentAppUsage): { paid: string; list: string | null; tip: string } {
  const paid = row.paid_by === "mixed" ? "plan and key" : (paidByLabel(row.paid_by) ?? "payer unknown");
  const list = row.notional_usd === null ? null : `≈ ${money(row.notional_usd)} at list prices`;
  const tip =
    row.paid_by === "plan"
      ? `Reported by ${row.name}. On your plan this isn't charged per job.`
      : row.paid_by === "api-key"
        ? `Reported by ${row.name}. This is billed to the API key you gave it, not through me.`
        : row.paid_by === "mixed"
          ? `Reported by ${row.name}. Some jobs ran on your plan and some on a key.`
          : `Reported by ${row.name}. It is not billed through me.`;
  return { paid, list, tip: list === null ? `${row.name} doesn't report a price.` : tip };
}

function Rows({
  rows,
  labelOf,
  empty,
}: {
  rows: api.UsageBucket[];
  labelOf: (row: api.UsageBucket) => string;
  empty: string;
}) {
  if (rows.length === 0) return <p className="usage-empty">{empty}</p>;
  return (
    <div className="usage-rows">
      {rows.map((row) => (
        <div key={row.key} className="usage-row">
          <span className="usage-name">{labelOf(row)}</span>
          <span className="usage-runs">
            {row.runs} {row.runs === 1 ? "run" : "runs"}
          </span>
          <span className="usage-tokens">{tokenLine(row)}</span>
          <span
            className={`usage-cost ${row.provenance === "local" ? "free" : ""}`}
          >
            {priceLine(row)}
          </span>
        </div>
      ))}
    </div>
  );
}

export default function Usage() {
  const [days, setDays] = useState<number>(30);
  const [data, setData] = useState<api.PricedUsage | null>(null);
  const [failed, setFailed] = useState(false);
  const setActiveConversation = useAppStore((s) => s.setActiveConversation);
  // `UCM-10`: `/usage` lands on this chat's own row.
  const usageFocus = useAppStore((s) => s.usageFocus);
  const focusRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    focusRef.current?.scrollIntoView?.({ block: "center" });
  }, [data, usageFocus]);

  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    setFailed(false);
    api
      .usageSummary(days)
      .then((d) => live && setData(d))
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [days]);

  // `CA-UI-8`: calls that connected apps ran. A count only: Composio bills per call
  // against the person's own plan, and I don't know their rate.
  const [appCalls, setAppCalls] = useState<api.AppUsage[]>([]);
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    api
      .composioUsage(days)
      .then((rows) => live && setAppCalls(rows))
      .catch(() => live && setAppCalls([]));
    return () => {
      live = false;
    };
  }, [days]);
  const appCallTotal = appCalls.reduce((n, r) => n + r.calls, 0);

  // `CTL-UI-2`: quick decisions, by where they were asked. Local ones cost nothing.
  const [decisions, setDecisions] = useState<api.DecisionUsageRow[]>([]);
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    api
      .decideUsage(days)
      .then((rows) => live && setDecisions(rows))
      .catch(() => live && setDecisions([]));
    return () => {
      live = false;
    };
  }, [days]);
  const decisionSums = decisionTotals(decisions);

  const total = data?.total;
  const hasAnything = (total?.runs ?? 0) > 0;
  const agentApps = data?.agent_apps ?? [];
  const hasApps = agentApps.length > 0;

  return (
    // The same shell every other tab in the hub uses (`.surface` /
    // `.surface-inner`, an `h1` and a lede). This panel used to render a bare
    // `.usage-panel` with its own heading level and no padding, so it sat in
    // the corner looking like a different application from the tab beside it.
    <div className="surface">
      <div className="surface-inner usage-panel">
        <h1>Usage</h1>
        <p className="lede">
          What runs have cost, by day, by model and by chat. A run is one turn
          of mine, tools and all — not one message.
        </p>

        <div className="usage-ranges">
          {RANGES.map((r) => (
            <button
              key={r.days}
              className={`usage-range ${days === r.days ? "active" : ""}`}
              onClick={() => setDays(r.days)}
            >
              {r.label}
            </button>
          ))}
        </div>

        {failed && (
          <p className="usage-empty">I could not read the usage records.</p>
        )}

        {/* Three ledgers each saying "nothing yet" is not an empty state, it is
          three empty tables. When there is genuinely nothing, say what this
          page is for and why it might be blank. */}
        {!failed && data && !hasAnything && !hasApps && (
          <div className="usage-blank">
            <p>
              Nothing to show for this range. Once you have used the agent, this
              is where every run appears — grouped by day, by model and by chat,
              with what it cost.
            </p>
            <p>
              Work done on your own machine is listed too, and marked as costing
              nothing. Try a longer range above if you have not used the agent
              today.
            </p>
          </div>
        )}

        {total && hasAnything && (
          <div className="usage-total">
            <span className="usage-total-tokens">
              {tokens(total.prompt_tokens + total.output_tokens)} tokens over{" "}
              {total.runs} {total.runs === 1 ? "run" : "runs"}
            </span>
            {total.cost_usd !== null && (
              <span className="usage-total-cost">
                {money(total.cost_usd)}
                {data?.some_prices_unknown
                  ? " and more I have no price for"
                  : ""}
              </span>
            )}
            {total.cost_usd === null && total.runs > 0 && (
              <span className="usage-total-cost free">
                nothing I can put a price on
              </span>
            )}
          </div>
        )}

        {hasAnything && (
          <>
            <section className="usage-section">
              <h3>By day</h3>
              <Rows
                rows={data?.by_day ?? []}
                labelOf={(r) => new Date(Number(r.key)).toLocaleDateString()}
                empty="Nothing yet in this range."
              />
            </section>

            <section className="usage-section">
              <h3>By model</h3>
              <Rows
                rows={data?.by_model ?? []}
                labelOf={(r) => r.key}
                empty="Nothing yet in this range."
              />
            </section>

            <section className="usage-section">
              <h3>By chat</h3>
              {(data?.by_conversation ?? []).length === 0 ? (
                <p className="usage-empty">Nothing yet in this range.</p>
              ) : (
                <div className="usage-rows">
                  {data!.by_conversation.map((row) => (
                    <button
                      key={row.key}
                      ref={row.key === usageFocus ? focusRef : undefined}
                      className={`usage-row link ${row.key === usageFocus ? "focused" : ""}`}
                      onClick={() =>
                        row.label && setActiveConversation(row.key)
                      }
                      disabled={!row.label}
                      title={
                        row.label
                          ? "Open this chat"
                          : "This chat has been deleted"
                      }
                    >
                      <span className="usage-name">
                        {row.label ?? "a chat I no longer have"}
                      </span>
                      <span className="usage-runs">
                        {row.runs} {row.runs === 1 ? "run" : "runs"}
                        {row.agents_runs
                          ? `, including ${row.agents_runs} agent ${row.agents_runs === 1 ? "run" : "runs"}`
                          : ""}
                      </span>
                      <span className="usage-tokens">{tokenLine(row)}</span>
                      <span
                        className={`usage-cost ${row.provenance === "local" ? "free" : ""}`}
                      >
                        {priceLine(row)}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </section>
          </>
        )}

        {hasApps && (
          <section className="usage-section" aria-label="Agent apps">
            <h3>Agent apps</h3>
            <p className="usage-empty">
              Jobs I handed to apps on your computer. They are paid for by the app's own plan or key,
              so they are not in the totals above.
            </p>
            <div className="usage-rows">
              {agentApps.map((row) => {
                const cost = appCost(row);
                return (
                  <div className="usage-row usage-app" key={row.name}>
                    <span className="usage-name">{row.name}</span>
                    <span className="usage-runs">
                      {row.runs} {row.runs === 1 ? "job" : "jobs"} · {duration(row.duration_ms)}
                    </span>
                    <span className="usage-tokens">{tokenLine(row)}</span>
                    <span className="usage-cost" title={cost.tip}>
                      {cost.paid}
                      {cost.list && <span className="usage-notional"> · {cost.list}</span>}
                    </span>
                  </div>
                );
              })}
            </div>
          </section>
        )}

        {decisionSums.total > 0 && (
          <section className="usage-section" aria-label="Decisions">
            <h3>Decisions</h3>
            <p className="usage-empty">{decisionSummary(decisionSums)}</p>
            <div className="usage-rows">
              {decisions.map((row) => (
                <div className="usage-row" key={`${row.backend}:${row.model}`}>
                  <span className="usage-name">{row.model}</span>
                  <span className="usage-runs">{decisionCountText(row)}</span>
                  <span className="usage-tokens">{tokens(row.input_tokens)} in</span>
                  <span className={`usage-cost ${row.backend === "local" ? "free" : ""}`}>{decisionCostText(row)}</span>
                </div>
              ))}
            </div>
          </section>
        )}

        {appCallTotal > 0 && (
          <section className="usage-section">
            <h3>Connected apps</h3>
            <p className="usage-empty">
              {appCallTotal} {appCallTotal === 1 ? "call" : "calls"} in this range. Composio counts these against your
              own plan.{" "}
              <button className="link-button inline" onClick={() => void openUrl("https://platform.composio.dev")}>
                See Composio's own count →
              </button>
            </p>
            <div className="usage-rows">
              {appCalls.map((r) => (
                <div className="usage-row" key={r.app}>
                  <span className="usage-name">{r.app_name}</span>
                  <span className="usage-runs">
                    {r.calls} {r.calls === 1 ? "call" : "calls"}
                  </span>
                </div>
              ))}
            </div>
          </section>
        )}

        <p className="usage-note">
          Prices come from a small built-in table and can go out of date. Where
          I have no price for a model I show its tokens and say so, rather than
          counting it as free.
        </p>
      </div>
    </div>
  );
}
