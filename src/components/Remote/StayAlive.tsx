import { useEffect, useState } from "react";
import { autostartSet, autostartStatus, getSetting, inTauri, setSetting } from "../../lib/api";

/** The key the backend reads (`tray::KEEP_RUNNING_KEY`). On unless it says `off`. */
export const KEEP_RUNNING_KEY = "app.keep_running";

/**
 * `REM-UI-2`: the two switches under Interface that let me stay reachable. Closing the window leaves
 * me in the tray, and I can start when the person signs in. Both are plain on/off lines, like the
 * notification switch next to them.
 */
export default function StayAlive() {
  const [keep, setKeep] = useState(true);
  const [start, setStart] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri()) return;
    getSetting(KEEP_RUNNING_KEY)
      .then((v) => setKeep(v !== "off"))
      .catch(() => {});
    autostartStatus()
      .then(setStart)
      .catch(() => {});
  }, []);

  function changeKeep(next: boolean) {
    setKeep(next);
    if (inTauri()) void setSetting(KEEP_RUNNING_KEY, next ? "on" : "off").catch(() => setKeep(!next));
  }

  function changeStart(next: boolean) {
    setStart(next);
    setError(null);
    if (!inTauri()) return;
    autostartSet(next)
      .then(setStart)
      .catch((e) => {
        setStart(!next);
        setError(String(e));
      });
  }

  return (
    <>
      <label className="toggle-line">
        <input type="checkbox" checked={keep} onChange={(e) => changeKeep(e.target.checked)} />
        <span>Keep running when I close the window, so I can still be reached from the tray</span>
      </label>
      <label className="toggle-line">
        <input type="checkbox" checked={start} onChange={(e) => changeStart(e.target.checked)} />
        <span>Start when I sign in to this computer</span>
      </label>
      {error && <p className="hw-note error">{error}</p>}
    </>
  );
}
