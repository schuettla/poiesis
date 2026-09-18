import { useCallback, useEffect, useState } from "react";
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
  inTauri,
  FIT_LABEL,
  type ImageSetupStatus,
  type ImageModel,
  type ImageCatalogEntry,
} from "../../lib/api";
import { useAppStore } from "../../lib/store";
import type { Model } from "../../lib/types";
import { classifyModelLink } from "../../lib/addLink";
import { CatalogRow, type CatalogState } from "../Models/ModelRows";
import "../../routes/Models.css";

export function formatBytes(bytes: number): string {
  const mb = bytes / 1048576;
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

/** A local diffusion file as a joint-list model, so it can be starred and
 * made the default like any other (`MOD-2`). The id is the one the local
 * backend reports for its active checkpoint. */
export function localImageAsModel(m: ImageModel): Model {
  return {
    id: `media:local/${m.path}`,
    name: m.name,
    provenance: "local",
    modality: "image",
    backendId: "local",
    backendLabel: "This PC",
    available: true,
  };
}

/** The local image library, catalog and downloads, shared by the Images &
 * video tab's groups and its add doors. Every mutation re-reads the store's
 * media list too, so the picker never offers a file that's gone. */
export function useImageLibrary() {
  const [status, setStatus] = useState<ImageSetupStatus | null>(null);
  const [models, setModels] = useState<ImageModel[]>([]);
  const [catalog, setCatalog] = useState<ImageCatalogEntry[]>([]);
  const [dlProg, setDlProg] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const [justAdded, setJustAdded] = useState<string | null>(null);
  const refreshMediaModels = useAppStore((s) => s.refreshMediaModels);

  const refresh = useCallback(async () => {
    if (!inTauri()) return;
    try {
      const [s, m] = await Promise.all([imageSetupStatus(), listImageModels()]);
      setStatus(s);
      setModels(m);
    } catch (e) {
      setError(String(e));
    }
    await refreshMediaModels();
  }, [refreshMediaModels]);

  useEffect(() => {
    if (!inTauri()) return;
    refresh();
    imageCatalog().then(setCatalog).catch(() => {});
  }, [refresh]);

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
      setJustAdded(filename);
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
      setJustAdded(c.components.length > 1 ? c.name : (c.components[0]?.filename ?? c.name));
      await refresh();
    } catch (e) {
      setError(`Couldn't download ${c.name}: ${e}`);
      clearProg(c.id);
    }
  }

  async function makeDefault(path: string) {
    await setDefaultImageModel(path).catch((e) => setError(String(e)));
    await refresh();
  }

  async function remove(m: ImageModel) {
    await deleteImageModel(m.path).catch((e) => setError(String(e)));
    await refresh();
  }

  async function pickModelFile() {
    const selected = await open({ directory: false, multiple: false });
    if (typeof selected !== "string") return;
    await setSetting("imagegen.model_path", selected);
    await refresh();
  }

  return {
    status,
    models,
    catalog,
    dlProg,
    error,
    setError,
    justAdded,
    setJustAdded,
    refresh,
    download,
    downloadFromCatalog,
    makeDefault,
    remove,
    pickModelFile,
  };
}

export type ImageLibrary = ReturnType<typeof useImageLibrary>;

/** Door 1 on the Images & video tab: the curated diffusion catalog. */
export function RecommendedImages({ lib }: { lib: ImageLibrary }) {
  const ownedFiles = new Set(lib.models.map((m) => m.name));
  return (
    <div className="model-rows catalog-list">
      {lib.catalog.map((c) => {
        const prog = lib.dlProg[c.id];
        // A single-file model is installed under its filename; a
        // multi-file one under its display name, from the manifest.
        const owned = ownedFiles.has(c.name) || ownedFiles.has(c.components[0]?.filename ?? "");
        const parts = c.components.length;
        const state: CatalogState = owned ? "owned" : prog === undefined ? "idle" : prog;
        return (
          <CatalogRow
            key={c.id}
            name={c.name}
            description={c.note}
            fit={c.fit}
            fitLabel={FIT_LABEL[c.fit]}
            size={c.size_label}
            // What it will actually be generated at. These differ sharply
            // between families (a distilled model at the wrong guidance scale
            // produces unusable images), so they are stated, not hidden.
            details={[
              `${c.profile.size}px`,
              `${c.profile.steps} steps`,
              `cfg ${c.profile.cfg_scale}`,
              c.vram_label,
              parts > 1 ? `${parts} files` : "",
            ]}
            state={state}
            onDownload={() => lib.downloadFromCatalog(c)}
          />
        );
      })}
    </div>
  );
}

/** Door 2 on the Images & video tab: a direct file link, or a file already on
 * disk ("point at my own file" moved in here from the old Advanced toggle). */
export function LinkImages({ lib }: { lib: ImageLibrary }) {
  const [input, setInput] = useState("");
  const kind = classifyModelLink(input, "media");
  const inFlight = Object.entries(lib.dlProg);

  async function add() {
    if (kind.kind !== "file") {
      lib.setError(kind.kind === "empty" ? null : kind.label);
      return;
    }
    setInput("");
    await lib.download(kind.url, kind.filename);
  }

  return (
    <div className="add-door-body">
      <div className="add-row">
        <input
          className="add-input"
          aria-label="Link to an image model file"
          placeholder="https://…/model.safetensors"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <button className="btn-primary" onClick={add} disabled={kind.kind !== "file"}>
          Download
        </button>
      </div>
      {kind.kind !== "empty" && (
        <p className={`link-kind ${kind.kind === "invalid" ? "bad" : ""}`}>{kind.label}</p>
      )}
      {inFlight.map(([name, pct]) => (
        <div className="dl-progress wide" key={name}>
          <div className="dl-bar" style={{ width: `${pct}%` }} />
          <span className="dl-pct">
            {name} — {pct}%
          </span>
        </div>
      ))}
      <p className="add-help">
        Already have a model file?{" "}
        <button className="link-button inline" onClick={lib.pickModelFile}>
          Point at my own file…
        </button>
      </p>
    </div>
  );
}
