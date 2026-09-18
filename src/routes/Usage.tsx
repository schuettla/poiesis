import { useEffect, useState } from "react";
import * as api from "../lib/api";
import { inTauri } from "../lib/api";
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
function tokenLine(row: api.UsageBucket): string {
  if (row.prompt_tokens === 0 && row.output_tokens === 0)
    return "tokens not reported";
  return `${tokens(row.prompt_tokens)} in · ${tokens(row.output_tokens)} out`;
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

  const total = data?.total;
  const hasAnything = (total?.runs ?? 0) > 0;

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
        {!failed && data && !hasAnything && (
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
                      className="usage-row link"
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

        <p className="usage-note">
          Prices come from a small built-in table and can go out of date. Where
          I have no price for a model I show its tokens and say so, rather than
          counting it as free.
        </p>
      </div>
    </div>
  );
}
