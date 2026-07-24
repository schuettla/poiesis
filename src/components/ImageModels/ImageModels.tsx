import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  imageSetupStatus,
  imageCatalog,
  listImageModels,
  downloadImageModel,
  setDefaultImageModel,
  deleteImageModel,
  setSetting,
  type ImageSetupStatus,
  type ImageModel,
  type ImageCatalogEntry,
} from "../../lib/api";
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

  async function refresh() {
    try {
      const [s, m] = await Promise.all([imageSetupStatus(), listImageModels()]);
      setStatus(s);
      setModels(m);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
    imageCatalog().then(setCatalog).catch(() => {});
  }, []);

  async function download(url: string, filename: string) {
    setError(null);
    setDlProg((p) => ({ ...p, [filename]: 0 }));
    try {
      await downloadImageModel(url, filename, (p) => {
        const pct = p.total ? Math.round((p.received / p.total) * 100) : 0;
        setDlProg((prev) => ({ ...prev, [filename]: pct }));
      });
      setDlProg((p) => {
        const next = { ...p };
        delete next[filename];
        return next;
      });
      await refresh();
    } catch (e) {
      setError(`Couldn't download ${filename}: ${e}`);
      setDlProg((p) => {
        const next = { ...p };
        delete next[filename];
        return next;
      });
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
            const prog = dlProg[c.filename];
            const owned = ownedFiles.has(c.filename);
            return (
              <div className="model-card" key={c.filename}>
                <div className="model-card-head">
                  <span className="model-name">{c.name}</span>
                </div>
                <p className="model-desc">{c.note}</p>
                <div className="model-meta">
                  <span className="model-size">{c.size_label}</span>
                </div>
                <div className="model-card-actions">
                  {owned ? (
                    <span className="owned-note">In your models</span>
                  ) : prog === undefined ? (
                    <button className="btn-download" onClick={() => download(c.url, c.filename)}>
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
