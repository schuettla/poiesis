import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  imageSetupStatus,
  installImageEngine,
  setSetting,
  type ImageSetupStatus,
  type DownloadProgress,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";

/** The "Image" tab of the Engine view: install / status of the local
 * stable-diffusion.cpp engine — the image-gen twin of the llama.cpp engine
 * management next to it. */
export default function ImageEngine() {
  const [status, setStatus] = useState<ImageSetupStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const refreshMediaModels = useAppStore((s) => s.refreshMediaModels);

  // The local image backend only counts as usable with *both* an engine
  // binary and a checkpoint on disk, so installing (or repointing) the engine
  // changes what the model chooser should be offering.
  async function refresh() {
    try {
      setStatus(await imageSetupStatus());
    } catch (e) {
      setError(String(e));
    }
    await refreshMediaModels();
  }

  useEffect(() => {
    refresh();
  }, []);

  async function install() {
    setBusy(true);
    setError(null);
    try {
      setStatus(await installImageEngine((p) => setProg(p)));
      await refreshMediaModels();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setProg(null);
    }
  }

  async function pickBinary() {
    const selected = await open({ directory: false, multiple: false });
    if (typeof selected !== "string") return;
    await setSetting("imagegen.binary_path", selected);
    await refresh();
  }

  const pct = prog?.total ? Math.round((prog.received / prog.total) * 100) : null;

  return (
    <>
      {error && <p className="hw-note error">{error}</p>}

      <section className="engine-card">
        <div className="engine-card-head">
          <h2 className="section-title">Image engine</h2>
          <span className={`engine-state-badge ${status?.engine_installed ? "running" : "idle"}`}>
            <span className="dot" aria-hidden="true" />
            {status?.engine_installed ? "Installed" : "Not installed"}
          </span>
        </div>
        <p className="engine-sub">
          The local <strong>stable-diffusion.cpp</strong> engine, matched to your GPU and downloaded
          automatically — the diffusion-model twin of the llama.cpp engine. Installed once, reused
          for every image.
        </p>
        {status?.engine_path && (
          <div className="hw-grid">
            <div className="hw-row">
              <span className="hw-label">Location</span>
              <span className="hw-value path">{status.engine_path}</span>
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
            <button className={status?.engine_installed ? "btn-secondary" : "btn-primary"} onClick={install}>
              {status?.engine_installed ? "Reinstall engine" : "Install image engine"}
            </button>
          </div>
        )}

        <button className="link-button" onClick={() => setAdvanced((v) => !v)}>
          {advanced ? "Hide advanced" : "Advanced — point at my own binary"}
        </button>
        {advanced && (
          <div className="engine-actions" style={{ marginTop: 8 }}>
            <button className="btn-secondary" onClick={pickBinary}>
              Choose engine binary…
            </button>
          </div>
        )}
        <p className="engine-hint">
          Get diffusion models under <strong>Models → Image</strong>.
        </p>
      </section>
    </>
  );
}
