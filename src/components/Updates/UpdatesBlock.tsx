import { useState } from "react";
import { useAppStore } from "../../lib/store";
import { restartApp } from "../../lib/api";
import { checkedAgo, formatBytes } from "../../lib/updates";

/**
 * The home of self-update (`UPD-UI-1`): everything else — the toast, the badge,
 * the arrival receipt — points here. Copy is AUTOUPDATE_PLAN §5, first person.
 */
export default function UpdatesBlock({ version }: { version: string }) {
  const state = useAppStore((s) => s.updateState);
  const autoCheck = useAppStore((s) => s.updateAutoCheck);
  const checkForUpdates = useAppStore((s) => s.checkForUpdates);
  const startUpdateInstall = useAppStore((s) => s.startUpdateInstall);
  const declineUpdate = useAppStore((s) => s.declineUpdate);
  const setUpdateAutoCheck = useAppStore((s) => s.setUpdateAutoCheck);
  // "Later" on a staged update is real: it applies on the next normal start.
  const [deferred, setDeferred] = useState(false);

  const have = version ? `Version ${version}` : "Browser preview";

  return (
    <section className="setting-block">
      <h2 className="setting-title">Updates</h2>
      <p className="setting-help">
        I check GitHub for a newer version of myself. I'll never install one without asking you.
      </p>

      <div className="update-state" aria-live="polite">
        {state.phase === "idle" && (
          <>
            <p className="setting-readout">{have}</p>
            <button className="btn-secondary" onClick={() => void checkForUpdates(true)}>
              Check for updates
            </button>
          </>
        )}

        {state.phase === "checking" && (
          <>
            <p className="setting-readout">Checking…</p>
            <button className="btn-secondary" disabled>
              Check for updates
            </button>
          </>
        )}

        {state.phase === "current" && (
          <>
            <p className="setting-readout">{`${have} — I'm up to date.`}</p>
            <p className="setting-readout">{`Checked ${checkedAgo(state.checkedAt)}`}</p>
            <button className="btn-secondary" onClick={() => void checkForUpdates(true)}>
              Check again
            </button>
          </>
        )}

        {state.phase === "available" && (
          <>
            <p className="setting-readout">{`Version ${state.info.version} is available.`}</p>
            {state.info.notes && <div className="update-notes">{state.info.notes}</div>}
            <div className="update-actions">
              <button className="btn-primary" onClick={() => void startUpdateInstall()}>
                Download and install
              </button>
              <button className="btn-secondary" onClick={declineUpdate}>
                Not now
              </button>
            </div>
          </>
        )}

        {state.phase === "downloading" && (
          <>
            <p className="setting-readout">
              {state.total
                ? `Downloading… ${formatBytes(state.downloaded)} of ${formatBytes(state.total)}`
                : `Downloading… ${formatBytes(state.downloaded)}`}
            </p>
            <div className="imggen-progress update-progress">
              <div className="dl-progress">
                <div
                  className="dl-bar"
                  style={{
                    width: state.total
                      ? `${Math.min(100, (state.downloaded / state.total) * 100)}%`
                      : "0%",
                  }}
                />
                <span className="dl-pct">
                  {state.total ? `${Math.floor((state.downloaded / state.total) * 100)}%` : ""}
                </span>
              </div>
            </div>
          </>
        )}

        {state.phase === "installing" && (
          <p className="setting-readout">{`Installing version ${state.info.version}… I'll restart in a moment.`}</p>
        )}

        {state.phase === "ready" && (
          <>
            <p className="setting-readout">
              {`Installed. I'll be version ${state.info.version} once I restart.`}
            </p>
            {deferred ? (
              <p className="setting-readout">
                That's fine — it's already in place, and takes effect the next time I start.
              </p>
            ) : (
              <div className="update-actions">
                <button className="btn-primary" onClick={() => void restartApp()}>
                  Restart now
                </button>
                <button className="btn-secondary" onClick={() => setDeferred(true)}>
                  Later
                </button>
              </div>
            )}
          </>
        )}

        {state.phase === "error" && (
          <>
            <p className="setting-readout">{`I couldn't check just now — ${state.message}.`}</p>
            <button className="btn-secondary" onClick={() => void checkForUpdates(true)}>
              Try again
            </button>
          </>
        )}
      </div>

      <label className="toggle-line">
        <input
          type="checkbox"
          checked={autoCheck}
          onChange={(e) => void setUpdateAutoCheck(e.target.checked)}
        />
        <span>Check when I start</span>
      </label>
    </section>
  );
}
