import { useEffect, useState } from "react";
import { getSetting, inTauri, setSetting } from "../../lib/api";

/** The key the backend reads (`waiting::NOTIFY_KEY`). On unless it says `off`. */
export const NOTIFY_WAITING_KEY = "notify.waiting";

/**
 * `TEAM-21`: the switch under Interface. When something waits for the person and Poiesis is not in
 * front of them, a desktop notification says who is waiting. It is the only notification there is.
 */
export default function NotifyWaiting() {
  const [on, setOn] = useState(true);

  useEffect(() => {
    if (!inTauri()) return;
    getSetting(NOTIFY_WAITING_KEY)
      .then((v) => setOn(v !== "off"))
      .catch(() => {});
  }, []);

  function change(next: boolean) {
    setOn(next);
    if (inTauri()) void setSetting(NOTIFY_WAITING_KEY, next ? "on" : "off").catch(() => setOn(!next));
  }

  return (
    <label className="toggle-line">
      <input type="checkbox" checked={on} onChange={(e) => change(e.target.checked)} />
      <span>Tell me when someone is waiting for me</span>
    </label>
  );
}
