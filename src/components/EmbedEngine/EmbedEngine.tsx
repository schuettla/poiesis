import { useEffect, useState } from "react";
import {
  embedEngineStatus,
  installEmbedEngine,
  removeEmbedEngine,
  embedCatalog,
  listEmbedModels,
  setDefaultEmbedModel,
  downloadEmbedModel,
  rerankEngineStatus,
  installRerankEngine,
  removeRerankEngine,
  setRerankEnabled,
  rerankCatalog,
  listRerankModels,
  setDefaultRerankModel,
  downloadRerankModel,
  type EmbedSetupStatus,
  type EmbedCatalogEntry,
  type RerankSetupStatus,
  type RerankCatalogEntry,
  type ModelEntry,
  type DownloadProgress,
} from "../../lib/api";

/** The "Recall" tab of the Engine view: install / status of the local recall
 * helper — a second, CPU-only `llama-server` that turns text into vectors for
 * recall and folder search.
 *
 * User-facing copy here follows SMP-8a: no *embedding*, *vector*, *index*,
 * *reranker* or *chunk* on screen, in either mode. */
export default function EmbedEngine() {
  const [status, setStatus] = useState<EmbedSetupStatus | null>(null);
  const [catalog, setCatalog] = useState<EmbedCatalogEntry[]>([]);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [s, m] = await Promise.all([embedEngineStatus(), listEmbedModels()]);
      setStatus(s);
      setModels(m);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    embedCatalog()
      .then(setCatalog)
      .catch((e) => setError(String(e)));
    refresh();
  }, []);

  // These can each fail partway through — a download that dies after the
  // engine was provisioned, a removal that clears the library but can't delete
  // a locked file — so the state is always re-read from the backend rather
  // than assumed from a return value.
  async function install() {
    setBusy("install");
    setError(null);
    try {
      await installEmbedEngine((p) => setProg(p));
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
      setProg(null);
    }
  }

  async function remove() {
    setBusy("remove");
    setError(null);
    try {
      await removeEmbedEngine();
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
    }
  }

  async function selectModel(entry: EmbedCatalogEntry) {
    setBusy(entry.name);
    setError(null);
    try {
      const existing = models.find((m) => m.name === entry.name);
      const model = existing ?? (await downloadEmbedModel(entry, (p) => setProg(p)));
      await setDefaultEmbedModel(model.id);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
      setProg(null);
    }
  }

  const pct = prog?.total ? Math.round((prog.received / prog.total) * 100) : null;
  // The model alone isn't enough: it runs inside the shared engine the chat
  // model uses. If someone cleared the runtimes folder, say so and offer the
  // repair rather than showing "Installed" and failing at first use.
  const ready = !!status?.model_installed && !!status?.engine_installed;
  const needsRepair = !!status?.model_installed && !status?.engine_installed;

  return (
    <>
      {error && <p className="hw-note error">{error}</p>}

      <section className="engine-card">
        <div className="engine-card-head">
          <h2 className="section-title">Recall</h2>
          <span className={`engine-state-badge ${status?.running ? "running" : "idle"}`}>
            <span className="dot" aria-hidden="true" />
            {status?.running
              ? "Running"
              : ready
                ? "Installed"
                : needsRepair
                  ? "Needs repair"
                  : "Not installed"}
          </span>
        </div>
        <p className="engine-sub">
          I use this to recall things by meaning instead of by keyword. It runs on the CPU, so it
          never takes memory from the model you chat with.
        </p>
        {status?.model_path && (
          <div className="hw-grid">
            <div className="hw-row">
              <span className="hw-label">Model</span>
              <span className="hw-value">{status.model_name}</span>
            </div>
          </div>
        )}
        {needsRepair && (
          <p className="hw-note">
            My engine files are missing, so I can't start this yet. Installing again will fetch
            them — the model you already downloaded stays put.
          </p>
        )}

        {busy && prog ? (
          <div className="dl-progress wide" style={{ marginTop: 12 }}>
            <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
            <span className="dl-pct">{prog.label}</span>
          </div>
        ) : (
          <div className="engine-actions">
            {ready ? (
              <button className="btn-secondary" onClick={remove} disabled={busy === "remove"}>
                {busy === "remove" ? "Removing…" : "Remove recall"}
              </button>
            ) : (
              <button className="btn-primary" onClick={install} disabled={busy === "install"}>
                {busy === "install"
                  ? "Installing…"
                  : needsRepair
                    ? "Repair recall"
                    : "Install recall"}
              </button>
            )}
          </div>
        )}
      </section>

      {status?.model_installed && (
        <section className="engine-card">
          <h2 className="section-title">Which model does the recalling</h2>
          <p className="engine-sub">
            Both run at full precision — the compressed versions get noticeably worse at finding
            the right thing.
          </p>
          <div className="backend-list">
            {catalog.map((entry) => {
              const installed = models.find((m) => m.name === entry.name);
              const active = !!installed?.is_default;
              return (
                <button
                  key={entry.name}
                  className={`backend-row ${active ? "active" : ""}`}
                  onClick={() => selectModel(entry)}
                  disabled={busy === entry.name}
                  aria-pressed={active}
                >
                  <span className="backend-radio" aria-hidden="true" />
                  <span className="backend-name">
                    {entry.name} · {entry.size_label}
                  </span>
                  {installed ? (
                    <span className="tag tag-installed">Installed</span>
                  ) : (
                    <span className="tag tag-download">Downloads on use</span>
                  )}
                  {busy === entry.name && <span className="tag">Applying…</span>}
                </button>
              );
            })}
          </div>
          <p className="engine-hint">
            {catalog.find((c) => c.name === status.model_name)?.note} Switching means I'll have to
            learn what I've already read again — nothing is lost, but I'll re-read it.
          </p>
        </section>
      )}

      <RerankCard recallReady={ready} />
    </>
  );
}

/** The second engine card (`RRK-UI-1`), beneath the recall helper above: an
 * optional, off-by-default second pass that re-reads the closest matches
 * before answering. Same state model as the card above it. */
function RerankCard({ recallReady }: { recallReady: boolean }) {
  const [status, setStatus] = useState<RerankSetupStatus | null>(null);
  const [catalog, setCatalog] = useState<RerankCatalogEntry[]>([]);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [s, m] = await Promise.all([rerankEngineStatus(), listRerankModels()]);
      setStatus(s);
      setModels(m);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    rerankCatalog()
      .then(setCatalog)
      .catch((e) => setError(String(e)));
    refresh();
  }, []);

  async function install() {
    setBusy("install");
    setError(null);
    try {
      await installRerankEngine((p) => setProg(p));
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
      setProg(null);
    }
  }

  async function remove() {
    setBusy("remove");
    setError(null);
    try {
      await removeRerankEngine();
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
    }
  }

  async function toggleEnabled(next: boolean) {
    setBusy("toggle");
    setError(null);
    try {
      await setRerankEnabled(next);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
    }
  }

  async function selectModel(entry: RerankCatalogEntry) {
    setBusy(entry.name);
    setError(null);
    try {
      const existing = models.find((m) => m.name === entry.name);
      const model = existing ?? (await downloadRerankModel(entry, (p) => setProg(p)));
      await setDefaultRerankModel(model.id);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      setBusy(null);
      setProg(null);
    }
  }

  const pct = prog?.total ? Math.round((prog.received / prog.total) * 100) : null;
  const ready = !!status?.model_installed && !!status?.engine_installed;

  return (
    <>
      {error && <p className="hw-note error">{error}</p>}

      <section className="engine-card">
        <div className="engine-card-head">
          <h2 className="section-title">Sharper matches</h2>
          <span className={`engine-state-badge ${status?.running ? "running" : "idle"}`}>
            <span className="dot" aria-hidden="true" />
            {status?.running ? "Running" : ready ? "Installed" : "Not installed"}
          </span>
        </div>

        {!recallReady ? (
          // RRK-UI-3: reranking without the recall helper is meaningless —
          // explain rather than just disabling the button.
          <p className="engine-sub">
            Install my recall engine first — there's nothing to re-read without it.
          </p>
        ) : (
          <>
            <p className="engine-sub">
              Re-reads the closest matches before answering. Slower, and it needs another 540 MB.
            </p>
            {status?.model_path && (
              <div className="hw-grid">
                <div className="hw-row">
                  <span className="hw-label">Model</span>
                  <span className="hw-value">{status.model_name}</span>
                </div>
              </div>
            )}

            {busy && prog ? (
              <div className="dl-progress wide" style={{ marginTop: 12 }}>
                <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
                <span className="dl-pct">{prog.label}</span>
              </div>
            ) : (
              <div className="engine-actions">
                {ready ? (
                  <>
                    <label className="toggle-line">
                      <input
                        type="checkbox"
                        checked={status?.enabled ?? false}
                        disabled={busy === "toggle"}
                        onChange={(e) => toggleEnabled(e.target.checked)}
                      />
                      <span>Use it when the best matches are close</span>
                    </label>
                    <button className="btn-secondary" onClick={remove} disabled={busy === "remove"}>
                      {busy === "remove" ? "Removing…" : "Remove"}
                    </button>
                  </>
                ) : (
                  <button className="btn-primary" onClick={install} disabled={busy === "install"}>
                    {busy === "install" ? "Installing…" : "Install re-reading"}
                  </button>
                )}
              </div>
            )}
          </>
        )}
      </section>

      {recallReady && status?.model_installed && (
        <section className="engine-card">
          <h2 className="section-title">Which model re-reads the matches</h2>
          <p className="engine-sub">
            Both run at full precision. The larger one is noticeably better at telling close
            matches apart, at the cost of a slower pass.
          </p>
          <div className="backend-list">
            {catalog.map((entry) => {
              const installed = models.find((m) => m.name === entry.name);
              const active = !!installed?.is_default;
              return (
                <button
                  key={entry.name}
                  className={`backend-row ${active ? "active" : ""}`}
                  onClick={() => selectModel(entry)}
                  disabled={busy === entry.name}
                  aria-pressed={active}
                >
                  <span className="backend-radio" aria-hidden="true" />
                  <span className="backend-name">
                    {entry.name} · {entry.size_label}
                  </span>
                  {installed ? (
                    <span className="tag tag-installed">Installed</span>
                  ) : (
                    <span className="tag tag-download">Downloads on use</span>
                  )}
                  {busy === entry.name && <span className="tag">Applying…</span>}
                </button>
              );
            })}
          </div>
          <p className="engine-hint">{catalog.find((c) => c.name === status.model_name)?.note}</p>
        </section>
      )}
    </>
  );
}
