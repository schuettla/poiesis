import { useCallback, useEffect, useMemo, useState } from "react";
import {
  decisionsList,
  inTauri,
  listActivity,
  type ActivityEntry,
  type DecisionEntry,
} from "../lib/api";
import {
  actionText,
  decisionLine,
  experimentsIn,
  filterRows,
  groupLine,
  labelText,
  optionsOf,
  rowsOf,
  wasSkipped,
  type Filter,
  type Row,
} from "../lib/decisions";
import { useAppStore } from "../lib/store";
import "./Surface.css";
import "./Settings.css";
import "./Activity.css";

const PAGE = 200;

/**
 * Activity: the log of everything Poiesis Agent did on this computer.
 *
 * It used to be one block near the bottom of General, where a list that only
 * grows had to share a page with settings that don't. It's a record you come to
 * read, so it gets its own section — last in the hub, after Tasks.
 *
 * `CTL-UI-1`: the quick decisions I made sit in the same list, one collapsed
 * line each. Background ones are folded into one line per hour.
 */
export default function Activity() {
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [decisions, setDecisions] = useState<DecisionEntry[]>([]);
  const [loading, setLoading] = useState(false);
  // A card's Details link opens this list already narrowed to its experiment.
  const [filter, setFilter] = useState<Filter>(() => (useAppStore.getState().activityFilter as Filter | null) ?? "all");
  const experiments = useAppStore((s) => s.experiments);
  useEffect(() => {
    useAppStore.getState().setActivityFilter(null);
  }, []);

  const load = useCallback(() => {
    if (!inTauri()) return;
    setLoading(true);
    Promise.all([
      listActivity(PAGE).then(setActivity).catch(() => {}),
      decisionsList(PAGE).then(setDecisions).catch(() => {}),
    ]).finally(() => setLoading(false));
  }, []);

  useEffect(load, [load]);

  const rows = useMemo(() => filterRows(rowsOf(activity, decisions), filter), [activity, decisions, filter]);
  const chips = useMemo(() => {
    const out: { id: Filter; label: string }[] = [{ id: "all", label: "Everything" }, { id: "activity", label: "Actions" }];
    if (decisions.length > 0) out.push({ id: "decisions", label: "Decisions" });
    for (const id of experimentsIn(decisions)) {
      out.push({ id: `experiment:${id}`, label: experiments?.find((e) => e.id === id)?.title ?? id });
    }
    return out;
  }, [decisions, experiments]);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Activity</h1>
        <p className="lede">
          Everything Poiesis Agent did on your computer, most recent first — files it read or
          wrote, sites it reached, tools it ran, and the quick decisions it made.
        </p>

        <div className="activity-head">
          <span className="activity-count">
            {rows.length === 0 ? "" : `${rows.length} ${rows.length === 1 ? "entry" : "entries"}`}
          </span>
          <button className="btn-secondary" onClick={load} disabled={loading}>
            {loading ? "Refreshing…" : "Refresh"}
          </button>
        </div>

        {chips.length > 2 && (
          <div className="activity-filters" role="group" aria-label="Show">
            {chips.map((c) => (
              <button
                key={c.id}
                className={`activity-chip ${filter === c.id ? "on" : ""}`}
                aria-pressed={filter === c.id}
                onClick={() => setFilter(c.id)}
              >
                {c.label}
              </button>
            ))}
          </div>
        )}

        {rows.length === 0 && (
          <p className="empty-hint">
            {inTauri() ? "No activity yet." : "Browser preview — no activity is recorded."}
          </p>
        )}

        <ul className="activity-list">
          {rows.map((r) => (
            <ActivityRow key={r.key} row={r} />
          ))}
        </ul>
      </div>
    </div>
  );
}

function ActivityRow({ row }: { row: Row }) {
  const [open, setOpen] = useState(false);
  if (row.kind === "activity") {
    const a = row.entry;
    return (
      <li className="activity-row">
        <span className={`activity-kind kind-${a.kind}`}>{a.kind}</span>
        <span className="activity-detail">{a.detail}</span>
        <span className="activity-time">{new Date(a.created_at).toLocaleString()}</span>
      </li>
    );
  }
  const text = row.kind === "group" ? groupLine(row) : decisionLine(row.entry);
  const entries = row.kind === "group" ? row.entries : [row.entry];
  return (
    <li className="activity-decision">
      <button className="activity-row activity-decision-line" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <span className="activity-kind kind-decide">{open ? "▾" : "▸"} decision</span>
        <span className="activity-detail">{text}</span>
        <span className="activity-time">{new Date(row.at).toLocaleString()}</span>
      </button>
      {open && (
        <div className="activity-decision-body">
          {entries.map((d) => (
            <DecisionDetail key={d.id} d={d} />
          ))}
        </div>
      )}
    </li>
  );
}

/** What was asked, a bar per option, what the controller did, and how it turned out. */
function DecisionDetail({ d }: { d: DecisionEntry }) {
  const options = optionsOf(d);
  const action = actionText(d.action);
  return (
    <div className="decision-detail">
      <p className="decision-q">{d.label || d.purpose}</p>
      {d.state_preview && <p className="decision-state">{d.state_preview}</p>}
      {wasSkipped(d) ? (
        <p className="decision-action">{action || "Not asked."}</p>
      ) : (
        <>
          <ul className="decision-bars">
            {options.map((o) => (
              <li key={o.name} className="decision-bar">
                <span className="decision-opt">{o.name}</span>
                <span className="decision-track" aria-hidden="true">
                  <span className="decision-fill" style={{ width: `${Math.round(o.p * 100)}%` }} />
                </span>
                <span className="decision-p">{o.p.toFixed(2)}</span>
              </li>
            ))}
          </ul>
          <p className="decision-action">
            {action}
            {action && !d.calibrated ? " · " : ""}
            {!d.calibrated && "Not calibrated yet: these numbers can only make me more careful"}
          </p>
        </>
      )}
      {d.labels.length > 0 && (
        <p className="decision-outcome">
          Afterwards: {d.labels.map((l) => labelText(l)).join(", ")}
        </p>
      )}
    </div>
  );
}
