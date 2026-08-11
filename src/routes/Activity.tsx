import { useCallback, useEffect, useState } from "react";
import { inTauri, listActivity, type ActivityEntry } from "../lib/api";
import "./Surface.css";
import "./Settings.css";

const PAGE = 200;

/**
 * Activity: the log of everything Poiesis Agent did on this computer.
 *
 * It used to be one block near the bottom of General, where a list that only
 * grows had to share a page with settings that don't. It's a record you come to
 * read, so it gets its own section — last in the hub, after Tasks.
 */
export default function Activity() {
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [loading, setLoading] = useState(false);

  const load = useCallback(() => {
    if (!inTauri()) return;
    setLoading(true);
    listActivity(PAGE)
      .then(setActivity)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  useEffect(load, [load]);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Activity</h1>
        <p className="lede">
          Everything Poiesis Agent did on your computer, most recent first — files it read or
          wrote, sites it reached, tools it ran.
        </p>

        <div className="activity-head">
          <span className="activity-count">
            {activity.length === 0
              ? ""
              : `${activity.length} ${activity.length === 1 ? "entry" : "entries"}`}
          </span>
          <button className="btn-secondary" onClick={load} disabled={loading}>
            {loading ? "Refreshing…" : "Refresh"}
          </button>
        </div>

        {activity.length === 0 && (
          <p className="empty-hint">
            {inTauri() ? "No activity yet." : "Browser preview — no activity is recorded."}
          </p>
        )}

        <ul className="activity-list">
          {activity.map((a) => (
            <li key={a.id} className="activity-row">
              <span className={`activity-kind kind-${a.kind}`}>{a.kind}</span>
              <span className="activity-detail">{a.detail}</span>
              <span className="activity-time">{new Date(a.created_at).toLocaleString()}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
