import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  imageSetupStatus,
  imageCatalog,
  listImageModels,
  downloadImageModel,
  downloadImageCatalogModel,
  setDefaultImageModel,
  deleteImageModel,
  setSetting,
  FIT_LABEL,
  type ImageSetupStatus,
  type ImageModel,
  type ImageCatalogEntry,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import "../../routes/Models.css";

function formatSize(bytes: number): string {
  const mb = bytes / 1048576;
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

/** The "Image" tab of the Models view: the image engine + diffusion model library
 * (choose from a catalog, add by URL, set default, delete) — the image-gen twin
 * of the language-model marketplace. */
export default function ImageModels() {
  const [status, setStatus] = useState<ImageSetupStatus | null>(null);
  const [models, setModels] = useState<ImageModel[]>([]);
  const [catalog, setCatalog] = useState<ImageCatalogEntry[]>([]);
  const [dlProg, setDlProg] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const [urlInput, setUrlInput] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const refreshMediaModels = useAppStore((s) => s.refreshMediaModels);

  // Every mutation below routes through here, so this is also where the
  // *shared* model list gets re-read. Without that, deleting or adding a
  // diffusion model only updated this screen's own state — the model chooser
  // went on offering a checkpoint that was no longer on disk until the app
  // was restarted.
  async function refresh() {
    try {
      const [s, m] = await Promise.all([imageSetupStatus(), listImageModels()]);
      setStatus(s);
      setModels(m);
    } catch (e) {
      setError(String(e));
    }
    await refreshMediaModels();
  }

  useEffect(() => {
    refresh();
    imageCatalog().then(setCatalog).catch(() => {});
  }, []);

  function clearProg(key: string) {
    setDlProg((p) => {
      const next = { ...p };
      delete next[key];
      return next;
    });
  }

  async function download(url: string, filename: string) {
    setError(null);
    setDlProg((p) => ({ ...p, [filename]: 0 }));
    try {
      await downloadImageModel(url, filename, (p) => {
        const pct = p.total ? Math.round((p.received / p.total) * 100) : 0;
        setDlProg((prev) => ({ ...prev, [filename]: pct }));
      });
      clearProg(filename);
      await refresh();
    } catch (e) {
      setError(`Couldn't download ${filename}: ${e}`);
      clearProg(filename);
    }
  }

  /** Catalog downloads go by id so the backend owns the file list — a
   * multi-file model reports one combined percentage across all its parts. */
  async function downloadFromCatalog(c: ImageCatalogEntry) {
    setError(null);
    setDlProg((p) => ({ ...p, [c.id]: 0 }));
    try {
      await downloadImageCatalogModel(c.id, (p) => {
        const pct = p.total ? Math.round((p.received / p.total) * 100) : 0;
        setDlProg((prev) => ({ ...prev, [c.id]: pct }));
      });
      clearProg(c.id);
      await refresh();
    } catch (e) {
      setError(`Couldn't download ${c.name}: ${e}`);
      clearProg(c.id);
    }
  }

  async function addByUrl() {
    const url = urlInput.trim();
    if (!/^https?:\/\//i.test(url)) {
      setError("Enter a direct https link to a .safetensors, .gguf, or .ckpt file.");
      return;
    }
    const filename = url.split("?")[0].split("/").pop() || "model.safetensors";
    setUrlInput("");
    await download(url, filename);
  }

  async function makeDefault(path: string) {
    await setDefaultImageModel(path).catch((e) => setError(String(e)));
    await refresh();
  }

  async function remove(m: ImageModel) {
    if (!confirm(`Remove "${m.name}" and delete its file from disk?`)) return;
    await deleteImageModel(m.path).catch((e) => setError(String(e)));
    await refresh();
  }

  async function pickModelFile() {
    const selected = await open({ directory: false, multiple: false });
    if (typeof selected !== "string") return;
    await setSetting("imagegen.model_path", selected);
    await refresh();
  }

  const ownedFiles = new Set(models.map((m) => m.name));

  return (
    <>
      {error && <p className="hw-note error">{error}</p>}

      {status && !status.engine_installed && (
        <p className="add-help">
          Tip: install the image engine under <strong>Engine → Image</strong> so downloaded models
          can generate.
        </p>
      )}

      {/* Installed models */}
      {models.length > 0 && (
        <section className="model-section">
          <h2 className="section-title">Your image models</h2>
          <div className="card-grid">
            {models.map((m) => (
              <div className="model-card" key={m.path}>
                <div className="model-card-head">
                  <span className="model-name">{m.name}</span>
                </div>
                <div className="model-meta">
                  {formatSize(m.size_bytes)}
                  {m.is_default ? " · default" : ""}
                </div>
                <div className="model-card-actions">
                  {!m.is_default && (
                    <button className="btn-text" onClick={() => makeDefault(m.path)}>
                      Set default
                    </button>
                  )}
                  <button className="btn-text danger" onClick={() => remove(m)}>
                    Delete
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Catalog */}
      <section className="model-section">
        <h2 className="section-title">Get a model</h2>
        <div className="card-grid">
          {catalog.map((c) => {
            const prog = dlProg[c.id];
            // A single-file model is installed under its filename; a
            // multi-file one under its display name, from the manifest.
            const owned =
              ownedFiles.has(c.name) || ownedFiles.has(c.components[0]?.filename ?? "");
            const parts = c.components.length;
            return (
              <div className="model-card" key={c.id}>
                <div className="model-card-head">
                  <span className="model-name">{c.name}</span>
                </div>
                <p className="model-desc">{c.note}</p>
                <div className="model-meta">
                  <span className={`fit-badge fit-${c.fit}`}>{FIT_LABEL[c.fit]}</span>
                  <span className="model-size">{c.size_label}</span>
                  {parts > 1 && <span className="model-parts">{parts} files</span>}
                </div>
                {/* What it will actually be generated at. These differ sharply
                    between families — a distilled model at the wrong guidance
                    scale produces unusable images — so they are stated up
                    front rather than hidden in the engine's defaults. */}
                <div className="model-speed">
                  {c.profile.size}px · {c.profile.steps} steps · cfg {c.profile.cfg_scale} ·{" "}
                  {c.vram_label}
                </div>
                <div className="model-card-actions">
                  {owned ? (
                    <span className="owned-note">In your models</span>
                  ) : prog === undefined ? (
                    <button
                      className="btn-download"
                      disabled={c.fit === "wont-fit"}
                      onClick={() => downloadFromCatalog(c)}
                    >
                      Download
                    </button>
                  ) : (
                    <div className="dl-progress">
                      <div className="dl-bar" style={{ width: `${prog}%` }} />
                      <span className="dl-pct">{prog}%</span>
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {/* Add by URL */}
      <section className="model-section">
        <h2 className="section-title">Add a model by link</h2>
        <p className="add-help">
          Paste a direct link to a <code>.safetensors</code>, <code>.gguf</code>, or{" "}
          <code>.ckpt</code> diffusion model.
        </p>
        <div className="add-row">
          <input
            className="add-input"
            placeholder="https://…/model.safetensors"
            value={urlInput}
            onChange={(e) => setUrlInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addByUrl()}
          />
          <button className="btn-primary" onClick={addByUrl} disabled={!urlInput.trim()}>
            Download
          </button>
        </div>

        <button className="link-button" onClick={() => setAdvanced((v) => !v)}>
          {advanced ? "Hide advanced" : "Advanced — point at my own file"}
        </button>
        {advanced && (
          <div className="add-row" style={{ marginTop: 10, gap: 10 }}>
            <button className="btn-secondary" onClick={pickModelFile}>
              Choose model file…
            </button>
          </div>
        )}
      </section>
    </>
  );
}
