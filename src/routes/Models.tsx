import { useEffect, useState } from "react";
import {
  detectHardware,
  recommendRuntime,
  recommendedCatalog,
  listRepoFiles,
  listGithubModels,
  listModels,
  setDefaultModel,
  deleteModelEntry,
  inTauri,
  FIT_LABEL,
  type HardwareProfile,
  type RuntimeSelection,
  type CatalogEntry,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import ImageModels from "../components/ImageModels/ImageModels";
import "./Surface.css";
import "./Models.css";

function formatVram(mb: number | null): string {
  if (!mb) return "";
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1)} GB` : `${mb} MB`;
}
function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}
interface RepoGroup {
  repo: string;
  files: CatalogEntry[];
  pick: number;
}

export default function Models() {
  const [tab, setTab] = useState<"language" | "image">("language");
  const [hw, setHw] = useState<HardwareProfile | null>(null);
  const [rec, setRec] = useState<RuntimeSelection | null>(null);
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [discovered, setDiscovered] = useState<RepoGroup[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [repoInput, setRepoInput] = useState("");
  const [finding, setFinding] = useState(false);

  const libraryModels = useAppStore((s) => s.libraryModels);
  const refreshLibrary = useAppStore((s) => s.refreshLibrary);
  const loadModelById = useAppStore((s) => s.loadModelById);
  const loadingModel = useAppStore((s) => s.loadingModel);
  const selectedModelId = useAppStore((s) => s.selectedModelId);
  const setView = useAppStore((s) => s.setView);
  // Lives in the store, not local state, so it survives leaving and
  // returning to this view instead of resetting to a bare "Download" button.
  const progress = useAppStore((s) => s.modelDownloads);
  const downloadCatalogModel = useAppStore((s) => s.downloadCatalogModel);

  const haveByUrl = new Set(libraryModels.map((m) => m.path.split(/[\\/]/).pop()));
  const filenameOf = (entry: CatalogEntry) => entry.url.split("?")[0].split("/").pop() ?? "";
  const totalMb = libraryModels.reduce((s, m) => s + (m.size_bytes ?? 0), 0) / 1048576;
  const isFirstRun = inTauri() && libraryModels.length === 0;
  const firstPick = catalog.find((e) => e.fit !== "wont-fit") ?? catalog[0];

  useEffect(() => {
    if (!inTauri()) return;
    Promise.all([detectHardware(), recommendRuntime(), recommendedCatalog()])
      .then(([h, r, c]) => {
        setHw(h);
        setRec(r);
        setCatalog(c);
      })
      .catch((e) => setError(String(e)));
  }, []);

  async function download(entry: CatalogEntry) {
    try {
      await downloadCatalogModel(entry);
    } catch (e) {
      setError(String(e));
    }
  }

  // Add by Hugging Face repo, GitHub owner/repo, or a direct .gguf link (MKT-6).
  async function addByRepo() {
    const input = repoInput.trim();
    if (!input) return;
    setError(null);
    setFinding(true);
    try {
      if (/^https?:\/\//i.test(input) && input.split("?")[0].toLowerCase().endsWith(".gguf")) {
        // Direct GGUF URL — download straight away.
        const name = input.split("?")[0].split("/").pop() ?? "model.gguf";
        await download({
          id: `url:${input}`,
          name: name.replace(/\.gguf$/i, ""),
          description: "",
          quant: "",
          size_mb: 0,
          vision: false,
          url: input,
          source: "url",
          license: null,
          fit: "great",
          speed: "",
        });
        setRepoInput("");
        return;
      }

      const isGithub = input.includes("github.com");
      const repoId = isGithub
        ? input.replace(/^https?:\/\/github\.com\//i, "").split("/").slice(0, 2).join("/")
        : input;
      const files = isGithub ? await listGithubModels(repoId) : await listRepoFiles(repoId);
      if (files.length === 0) {
        setError(`No GGUF files found in ${repoId}.`);
        return;
      }
      const sorted = [...files].sort((a, b) => a.size_mb - b.size_mb);
      setDiscovered((d) => [{ repo: repoId, files: sorted, pick: Math.floor(sorted.length / 2) }, ...d]);
      setRepoInput("");
    } catch (e) {
      setError(`Couldn't read ${input}: ${e}`);
    } finally {
      setFinding(false);
    }
  }

  async function use(modelId: string) {
    setView("chat");
    await loadModelById(modelId).catch((e) => setError(String(e)));
  }

  async function setDefault(id: string) {
    await setDefaultModel(id).catch((e) => setError(String(e)));
    await refreshLibrary();
  }

  async function remove(id: string, name: string) {
    if (!confirm(`Remove "${name}" and delete its file from disk?`)) return;
    await deleteModelEntry(id).catch((e) => setError(String(e)));
    await refreshLibrary();
  }

  // First-run: detect → recommend → one click → into chat (5.4.1).
  async function getStarted(entry: CatalogEntry) {
    await download(entry);
    try {
      const fresh = await listModels();
      const file = filenameOf(entry);
      const m = fresh.find((x) => x.path.split(/[\\/]/).pop() === file);
      if (m) await use(m.id);
    } catch (e) {
      setError(String(e));
    }
  }

  function pickFor(repo: string, idx: number) {
    setDiscovered((d) => d.map((g) => (g.repo === repo ? { ...g, pick: idx } : g)));
  }

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Models</h1>
        <p className="lede">
          Browse open-weight models matched to your PC, download with one click, and manage your
          local library.
        </p>

        {/* Above the tabs on purpose: the same machine runs both kinds of
            model, and both catalogs judge their downloads against it. Nesting
            it inside the Language tab hid it exactly when someone was sizing
            up a 16 GB diffusion model. */}
        <section className="hw-panel">
          <h2 className="hw-title">Your PC</h2>
          {!inTauri() && <p className="hw-note">Hardware detection runs in the desktop app.</p>}
          {error && <p className="hw-note error">{error}</p>}
          {hw && (
            <div className="hw-grid">
              <div className="hw-row">
                <span className="hw-label">Processor</span>
                <span className="hw-value">
                  {hw.cpu.brand} · {hw.cpu.physical_cores} cores
                  {hw.cpu.avx512 ? " · AVX-512" : hw.cpu.avx2 ? " · AVX2" : ""}
                </span>
              </div>
              <div className="hw-row">
                <span className="hw-label">Memory</span>
                <span className="hw-value">{(hw.ram_mb / 1024).toFixed(1)} GB RAM</span>
              </div>
              {hw.gpus.map((g, i) => (
                <div className="hw-row" key={i}>
                  <span className="hw-label">Graphics</span>
                  <span className="hw-value">
                    {g.name}
                    {g.vram_mb ? ` · ${formatVram(g.vram_mb)}` : ""}
                  </span>
                </div>
              ))}
              {rec && (
                <div className="hw-row">
                  <span className="hw-label">Engine</span>
                  <span className="hw-value">{rec.rationale}</span>
                </div>
              )}
            </div>
          )}
        </section>

        <div className="model-tabs" role="tablist" aria-label="Model type">
          <button
            className={`model-tab ${tab === "language" ? "on" : ""}`}
            role="tab"
            aria-selected={tab === "language"}
            onClick={() => setTab("language")}
          >
            Language
          </button>
          <button
            className={`model-tab ${tab === "image" ? "on" : ""}`}
            role="tab"
            aria-selected={tab === "image"}
            onClick={() => setTab("image")}
          >
            Image
          </button>
        </div>

        {tab === "image" && <ImageModels />}

        {tab === "language" && (
          <>
        {/* First-run welcome (5.4.1) */}
        {isFirstRun && firstPick && (
          <section className="first-run">
            <h2 className="first-run-title">Get started in one step</h2>
            <p className="first-run-body">
              Based on your hardware, we recommend <strong>{firstPick.name}</strong> ({firstPick.quant}
              , {formatSize(firstPick.size_mb)}). {FIT_LABEL[firstPick.fit]} · {firstPick.speed}.
            </p>
            {progress[firstPick.id] !== undefined && progress[firstPick.id] !== "done" ? (
              <div className="dl-progress wide">
                <div className="dl-bar" style={{ width: `${progress[firstPick.id]}%` }} />
                <span className="dl-pct">Getting your model ready — {progress[firstPick.id]}%</span>
              </div>
            ) : (
              <button className="btn-primary big" onClick={() => getStarted(firstPick)}>
                Download &amp; start chatting
              </button>
            )}
          </section>
        )}

        {/* Your library (MKT-5) */}
        {libraryModels.length > 0 && (
          <section className="model-section">
            <div className="section-head">
              <h2 className="section-title">On this device</h2>
              <span className="disk-total">{formatSize(totalMb)} on disk</span>
            </div>
            <div className="card-grid">
              {libraryModels.map((m) => {
                const active = m.id === selectedModelId;
                const loading = loadingModel?.id === m.id;
                return (
                  <div className="model-card" key={m.id}>
                    <div className="model-card-head">
                      <span className="model-name">{m.name}</span>
                      {m.quant && <span className="model-quant">{m.quant}</span>}
                    </div>
                    <div className="model-meta">
                      {m.size_bytes ? formatSize(Math.round(m.size_bytes / 1048576)) : ""}
                      {m.is_default ? " · default" : ""}
                    </div>
                    <div className="model-card-actions">
                      <button className="btn-use" disabled={loading} onClick={() => use(m.id)}>
                        {loading ? loadingModel?.label : active ? "In use" : "Use"}
                      </button>
                      {!m.is_default && (
                        <button className="btn-text" onClick={() => setDefault(m.id)}>
                          Set default
                        </button>
                      )}
                      <button className="btn-text danger" onClick={() => remove(m.id, m.name)}>
                        Delete
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        )}

        {/* Recommended (MKT-1, MKT-4) */}
        <section className="model-section">
          <h2 className="section-title">Recommended for you</h2>
          <div className="card-grid">
            {catalog.map((entry) => {
              const prog = progress[entry.id];
              const owned = haveByUrl.has(filenameOf(entry));
              return (
                <div className="model-card" key={entry.id}>
                  <div className="model-card-head">
                    <span className="model-name">{entry.name}</span>
                    <span className="model-quant">{entry.quant}</span>
                  </div>
                  <p className="model-desc">{entry.description}</p>
                  <div className="model-meta">
                    <span className={`fit-badge fit-${entry.fit}`}>{FIT_LABEL[entry.fit]}</span>
                    <span className="model-size">{formatSize(entry.size_mb)}</span>
                  </div>
                  {entry.speed && <div className="model-speed">{entry.speed}</div>}
                  <div className="model-card-actions">
                    {owned ? (
                      <span className="owned-note">In your library</span>
                    ) : prog === undefined ? (
                      <button
                        className="btn-download"
                        disabled={entry.fit === "wont-fit"}
                        onClick={() => download(entry)}
                      >
                        Download
                      </button>
                    ) : prog === "done" ? (
                      <span className="owned-note">Downloaded</span>
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

        {/* Discovered repos with a quant slider (MKT-2, 5.4.2) */}
        {discovered.map((g) => {
          const file = g.files[g.pick];
          const prog = progress[file.id];
          const owned = haveByUrl.has(filenameOf(file));
          return (
            <section className="model-section" key={g.repo}>
              <h2 className="section-title">{g.repo}</h2>
              <div className="quant-card">
                <input
                  type="range"
                  className="quant-slider"
                  min={0}
                  max={g.files.length - 1}
                  value={g.pick}
                  aria-label="Choose quantization"
                  onChange={(e) => pickFor(g.repo, Number(e.target.value))}
                />
                <div className="quant-scale">
                  <span>Smaller · faster</span>
                  <span>Larger · higher quality</span>
                </div>
                <div className="quant-detail">
                  <span className="model-quant">{file.quant || "GGUF"}</span>
                  <span className="model-size">{formatSize(file.size_mb)}</span>
                  <span className={`fit-badge fit-${file.fit}`}>{FIT_LABEL[file.fit]}</span>
                  {file.speed && <span className="model-speed">{file.speed}</span>}
                </div>
                <div className="model-card-actions">
                  {owned ? (
                    <span className="owned-note">In your library</span>
                  ) : prog === undefined ? (
                    <button
                      className="btn-download"
                      disabled={file.fit === "wont-fit"}
                      onClick={() => download(file)}
                    >
                      Download {file.quant}
                    </button>
                  ) : prog === "done" ? (
                    <span className="owned-note">Downloaded</span>
                  ) : (
                    <div className="dl-progress">
                      <div className="dl-bar" style={{ width: `${prog}%` }} />
                      <span className="dl-pct">{prog}%</span>
                    </div>
                  )}
                </div>
              </div>
            </section>
          );
        })}

        {/* Add by link (MKT-6) */}
        <section className="model-section">
          <h2 className="section-title">Add a model</h2>
          <p className="add-help">
            Paste a Hugging Face repo (e.g. <code>bartowski/Qwen2.5-7B-Instruct-GGUF</code>), a
            GitHub <code>owner/repo</code>, or a direct <code>.gguf</code> link.
          </p>
          <div className="add-row">
            <input
              className="add-input"
              placeholder="owner/repo  or  https://…/model.gguf"
              value={repoInput}
              onChange={(e) => setRepoInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addByRepo()}
            />
            <button className="btn-primary" onClick={addByRepo} disabled={!repoInput.trim() || finding}>
              {finding ? "Finding…" : "Find files"}
            </button>
          </div>
        </section>
          </>
        )}
      </div>
    </div>
  );
}
