import { useEffect, useState } from "react";
import { dataDirOverview, formatDiskSize, inTauri, openPath, revealPath, type DataDirOverview } from "../lib/api";
import "./Surface.css";
import "./Settings.css";

/** What's inside Poiesis's own app-data folder (models, memory, skills, the
 * database, generated media, …) and how much room it takes — split out of
 * General so the folder is easy to find without digging through Explorer. */
export default function WorkingDir() {
  const [overview, setOverview] = useState<DataDirOverview | null>(null);
  const [loading, setLoading] = useState(true);

  function refresh() {
    if (!inTauri()) return;
    setLoading(true);
    dataDirOverview()
      .then(setOverview)
      .finally(() => setLoading(false));
  }

  useEffect(refresh, []);

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Working dir</h1>
        <p className="lede">Where Poiesis Agent keeps everything on your machine — models, memory, skills, and its database.</p>

        <section className="setting-block">
          <h2 className="setting-title">Folder</h2>
          {overview && (
            <>
              <p className="setting-readout wrap">{overview.path}</p>
              <p className="setting-help">Total size: {formatDiskSize(overview.total_bytes)}</p>
            </>
          )}
          {inTauri() && overview && (
            <div className="setting-actions">
              <button className="btn-secondary" onClick={() => revealPath(overview.path)}>
                Show in Explorer
              </button>
              <button className="btn-secondary" onClick={refresh} disabled={loading}>
                {loading ? "Refreshing…" : "Refresh"}
              </button>
            </div>
          )}
        </section>

        <section className="setting-block">
          <h2 className="setting-title">Contents</h2>
          <p className="setting-help">Largest first. Folders are totalled recursively.</p>
          {loading && <p className="empty-hint">Reading the folder…</p>}
          {!loading && overview && overview.entries.length === 0 && (
            <p className="empty-hint">Nothing here yet.</p>
          )}
          {overview?.entries.map((e) => (
            <div className="grant-row" key={e.name}>
              <span className="grant-path">
                {e.name}
                {e.is_dir ? "/" : ""}
              </span>
              <span className="grant-mode">{formatDiskSize(e.size_bytes)}</span>
              {inTauri() && (
                <button className="grant-revoke" onClick={() => openPath(`${overview.path}\\${e.name}`)}>
                  Open
                </button>
              )}
            </div>
          ))}
        </section>
      </div>
    </div>
  );
}
