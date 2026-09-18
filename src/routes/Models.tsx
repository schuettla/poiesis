import { useEffect, useMemo, useState } from "react";
import {
  detectHardware,
  recommendedCatalog,
  runtimeOverview,
  listRepoFiles,
  listGithubModels,
  listModels,
  deleteModelEntry,
  inTauri,
  FIT_LABEL,
  type HardwareProfile,
  type CatalogEntry,
  type ModelEntry,
  type ImageModel,
} from "../lib/api";
import { useAppStore } from "../lib/store";
import type { Model } from "../lib/types";
import { isMediaModel, isChatModel, favoriteTabOf, defaultFor, type FavoriteTab } from "../lib/modelPrefs";
import { classifyModelLink, noGgufMessage } from "../lib/addLink";
import {
  CatalogRow,
  FavoritesSection,
  ModelRow,
  type CatalogState,
  StarButton,
  capabilityChips,
  useUseModel,
} from "../components/Models/ModelRows";
import {
  useImageLibrary,
  localImageAsModel,
  formatBytes,
  RecommendedImages,
  LinkImages,
  type ImageLibrary,
} from "../components/ImageModels/ImageModels";
import ConfirmDialog from "../components/Confirm/ConfirmDialog";
import "./Surface.css";
import "../components/ModelPicker/ModelPicker.css";
import "./Models.css";

function formatSize(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}
function formatVram(mb: number | null): string {
  if (!mb) return "";
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1)} GB` : `${mb} MB`;
}
const filenameOf = (url: string) => url.split("?")[0].split("/").pop() ?? "";

/** How many rows a cloud group shows before "show all". */
const GROUP_PREVIEW = 8;

type Door = "recommended" | "link" | "account";

interface RepoGroup {
  repo: string;
  files: CatalogEntry[];
  pick: number;
}

/** One line of hardware for the Recommended door (`RTM-6`: the full panel
 * lives on Runtime). */
function hardwareLine(hw: HardwareProfile): string {
  const gpu = hw.gpus.find((g) => g.vram_mb) ?? hw.gpus[0];
  const parts = [gpu ? `${gpu.name}${gpu.vram_mb ? ` · ${formatVram(gpu.vram_mb)}` : ""}` : hw.cpu.brand];
  parts.push(`${Math.round(hw.ram_mb / 1024)} GB RAM`);
  return parts.join(" · ");
}

/** A provider group's own search (`MOD-6`): only the long cloud lists need
 * one, so it lives in their header rather than over the whole page. */
function matches(m: Model, q: string): boolean {
  if (!q) return true;
  return `${m.name} ${m.cloudModel ?? ""}`.toLowerCase().includes(q);
}

/** `MOD-1`: the joint view. Everything Poiesis can think and draw with, on
 * this PC or through the user's accounts, in one list per tab. */
export default function Models() {
  const [tab, setTab] = useState<FavoriteTab>("chat");
  const [door, setDoor] = useState<Door | null>(null);
  const [hw, setHw] = useState<HardwareProfile | null>(null);
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [runtimeInstalled, setRuntimeInstalled] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [justAdded, setJustAdded] = useState<string | null>(null);

  const models = useAppStore((s) => s.models);
  const libraryModels = useAppStore((s) => s.libraryModels);
  const providers = useAppStore((s) => s.providers);
  const filter = useAppStore((s) => s.modelFilter);
  const setFilter = useAppStore((s) => s.setModelFilter);
  const source = useAppStore((s) => s.modelsSource);
  const setSource = useAppStore((s) => s.setModelsSource);
  const openProviders = useAppStore((s) => s.openProviders);
  const openRuntime = useAppStore((s) => s.openRuntime);
  const imageLib = useImageLibrary();

  useEffect(() => {
    if (!inTauri()) return;
    Promise.all([detectHardware(), recommendedCatalog()])
      .then(([h, c]) => {
        setHw(h);
        setCatalog(c);
      })
      .catch((e) => setError(String(e)));
    runtimeOverview()
      .then((ov) => setRuntimeInstalled(ov.installed))
      .catch(() => setRuntimeInstalled(null));
  }, []);

  // A source chip from Providers / Runtime (`MOD-6`) always lands on Chat,
  // where server and cloud chat models live.
  useEffect(() => {
    if (source && source.source !== "local") setTab("chat");
  }, [source]);

  const inTab = (m: Model) => (tab === "chat" ? isChatModel(m) : isMediaModel(m));
  const bySource = (m: Model) => {
    if (!source) return true;
    if (source.source === "local") return m.provenance === "local";
    if (source.source.startsWith("endpoint:")) return m.endpointId === source.source.slice(9);
    if (source.source.startsWith("cloud:")) {
      const p = source.source.slice(6);
      return m.provenance === "cloud" && (m.provider === p || m.backendId === p);
    }
    return true;
  };
  const byFilter = (m: Model) =>
    filter === "all" ? true : filter === "local" ? m.provenance !== "cloud" : m.provenance === "cloud";
  const shown = models.filter((m) => inTab(m) && bySource(m) && byFilter(m));

  // Groups (`MOD-1`): On this PC, one per server, one per provider.
  const serverGroups = useMemo(() => {
    const out = new Map<string, { label: string; id: string; models: Model[] }>();
    for (const m of shown.filter((m) => m.provenance === "endpoint")) {
      const id = m.endpointId ?? "";
      const g = out.get(id) ?? { label: m.endpointLabel ?? "Your server", id, models: [] };
      g.models.push(m);
      out.set(id, g);
    }
    return [...out.values()];
  }, [shown]);
  const cloudGroups = useMemo(() => {
    const out = new Map<string, { label: string; key: string; models: Model[] }>();
    for (const m of shown.filter((m) => m.provenance === "cloud")) {
      const key = m.backendId ?? m.provider ?? "cloud";
      const label = m.backendLabel ?? m.meta ?? key;
      const g = out.get(key) ?? { label, key, models: [] };
      g.models.push(m);
      out.set(key, g);
    }
    return [...out.values()];
  }, [shown]);

  const showLocal = filter !== "cloud" && (!source || source.source === "local");
  const chatLibrary = libraryModels.filter((m) => !m.role || m.role === "chat");
  const isFirstRun = inTauri() && tab === "chat" && libraryModels.length === 0 && !models.some(
    (m) => isChatModel(m) && (m.provenance === "cloud" || m.provenance === "endpoint")
  );
  const unconnected = providers.filter((p) => !p.key_set);

  // Favorites must be able to name local image files that aren't the active
  // checkpoint yet, so the media tab's list includes them.
  const favoriteModels =
    tab === "chat" ? models : [...models, ...imageLib.models.map(localImageAsModel)];

  return (
    <div className="surface">
      <div className="surface-inner">
        <h1>Models</h1>
        <p className="lede">
          Everything Poiesis can think and draw with — on this PC or through your accounts.
        </p>
        {error && <p className="hw-note error">{error}</p>}

        <div className="models-toolbar">
          <div className="model-tabs" role="tablist" aria-label="Model type">
            {(
              [
                ["chat", "Chat"],
                ["media", "Images & video"],
              ] as [FavoriteTab, string][]
            ).map(([id, label]) => (
              <button
                key={id}
                className={`model-tab ${tab === id ? "on" : ""}`}
                role="tab"
                aria-selected={tab === id}
                onClick={() => {
                  setTab(id);
                  setDoor(null);
                }}
              >
                {label}
              </button>
            ))}
          </div>
          <div className="models-filters" role="group" aria-label="Where models run">
            {(
              [
                ["all", "All"],
                ["local", "On this PC"],
                ["cloud", "Cloud"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                className={`filter-chip ${filter === id ? "active" : ""}`}
                aria-pressed={filter === id}
                onClick={() => setFilter(id)}
              >
                {label}
              </button>
            ))}
            {source && (
              <button
                className="filter-chip active source-chip"
                aria-label={`Showing ${source.label} only. Clear.`}
                onClick={() => setSource(null)}
              >
                {source.label} ×
              </button>
            )}
          </div>
        </div>

        {/* `MOD-8`: two equal ways in when there is nothing to chat with. */}
        {isFirstRun && (
          <FirstRun
            catalog={catalog}
            onPickLocal={() => setDoor("recommended")}
            onConnect={() => openProviders()}
            onAdded={setJustAdded}
            setError={setError}
          />
        )}

        <FavoritesSection tab={tab} models={favoriteModels} />

        {/* On this PC */}
        {showLocal && tab === "chat" && chatLibrary.length > 0 && (
          <LocalChatGroup entries={chatLibrary} justAdded={justAdded} onDismissAdded={() => setJustAdded(null)} />
        )}
        {showLocal && tab === "media" && imageLib.models.length > 0 && (
          <LocalImageGroup lib={imageLib} />
        )}

        {/* Your server · X */}
        {serverGroups.map((g) => (
          <section className="model-section" key={g.id}>
            <div className="section-head">
              <h2 className="section-title">
                <span className="provenance-dot endpoint" aria-hidden="true" /> Your server · {g.label}
              </h2>
              <button className="link-button" onClick={() => openRuntime("servers")}>
                Manage in Runtime →
              </button>
            </div>
            <div className="model-rows">
              {g.models.map((m) => (
                <ModelRow key={m.id} model={m} />
              ))}
            </div>
          </section>
        ))}

        {/* Via Provider */}
        {cloudGroups.map((g) => (
          <CloudGroup key={g.key} label={g.label} models={g.models} expandAll={!!source} />
        ))}

        {shown.length === 0 && !isFirstRun && (tab === "chat" ? chatLibrary.length === 0 : imageLib.models.length === 0) && (
          <p className="add-help">
            {source || filter !== "all"
              ? "Nothing matches. Clear the filter to see everything."
              : tab === "chat"
                ? "No chat models yet. Add one below."
                : "No image or video models yet. Add one below, or connect an account that makes them."}
          </p>
        )}

        {/* `MOD-7`: three equal ways a model gets into Poiesis. */}
        <section className="model-section">
          <h2 className="section-title">Add models</h2>
          <div className="add-doors">
            <DoorCard
              icon="⬇"
              title="Recommended for this PC"
              body={hw ? hardwareLine(hw) : "Picks matched to your hardware"}
              action="Browse"
              open={door === "recommended"}
              onClick={() => setDoor(door === "recommended" ? null : "recommended")}
            />
            <DoorCard
              icon="⧉"
              title={tab === "chat" ? "From Hugging Face or a link" : "From a link or a file"}
              body={
                tab === "chat"
                  ? "Any GGUF repo or file link"
                  : "A .safetensors, .gguf or .ckpt file"
              }
              action="Add"
              open={door === "link"}
              onClick={() => setDoor(door === "link" ? null : "link")}
            />
            <DoorCard
              icon="⌁"
              title="Connect an account"
              body={
                providers.length
                  ? providers.map((p) => p.name).join(", ")
                  : "OpenAI, Anthropic, OpenRouter…"
              }
              action="Choose"
              open={door === "account"}
              onClick={() => setDoor(door === "account" ? null : "account")}
            />
          </div>
          {runtimeInstalled === false && (door === "recommended" || door === "link") && (
            <p className="add-help">Your first download also sets up the local runtime (≈60 MB).</p>
          )}

          {door === "recommended" && tab === "chat" && (
            <RecommendedChat catalog={catalog} onAdded={setJustAdded} setError={setError} />
          )}
          {door === "recommended" && tab === "media" && <RecommendedImages lib={imageLib} />}
          {door === "link" && tab === "chat" && <LinkChat onAdded={setJustAdded} setError={setError} />}
          {door === "link" && tab === "media" && <LinkImages lib={imageLib} />}
          {door === "account" && (
            <div className="add-door-body account-door">
              {unconnected.length === 0 ? (
                <p className="add-help">
                  Every account is connected.{" "}
                  <button className="link-button inline" onClick={() => openProviders()}>
                    Manage them in Providers →
                  </button>
                </p>
              ) : (
                <>
                  {unconnected.map((p) => (
                    <button key={p.id} className="btn-secondary" onClick={() => openProviders(p.id)}>
                      {p.name}
                    </button>
                  ))}
                  <button className="link-button" onClick={() => openProviders()}>
                    Open Providers →
                  </button>
                </>
              )}
            </div>
          )}
          {imageLib.error && tab === "media" && <p className="hw-note error">{imageLib.error}</p>}
        </section>
      </div>
    </div>
  );
}

function DoorCard({
  icon,
  title,
  body,
  action,
  open,
  onClick,
}: {
  icon: string;
  title: string;
  body: string;
  action: string;
  open: boolean;
  onClick: () => void;
}) {
  return (
    <button className={`add-door ${open ? "open" : ""}`} aria-expanded={open} onClick={onClick}>
      <span className="add-door-title">
        <span aria-hidden="true">{icon}</span> {title}
      </span>
      <span className="add-door-body-text">{body}</span>
      <span className="add-door-action">{open ? "Close" : action}</span>
    </button>
  );
}

/** One local model in the page's row style. Size, quant, path and delete sit
 * under Details, as technical bits do everywhere on this page (`MOD-5`). */
function LocalRow({
  model,
  details,
  path,
  inStore,
  highlight,
  useLabel,
  useDisabled,
  onDelete,
  onMadeDefault,
}: {
  model: Model;
  details: string;
  path: string;
  /** Selectable right now (a local image file only is while it's active). */
  inStore: boolean;
  highlight: boolean;
  useLabel?: string;
  useDisabled?: boolean;
  onDelete: () => void;
  onMadeDefault?: () => void;
}) {
  const prefs = useAppStore((s) => s.modelPrefs);
  const selectedModelId = useAppStore((s) => s.selectedModelId);
  const makeDefault = useAppStore((s) => s.setDefaultModelPref);
  const use = useUseModel();
  const [open, setOpen] = useState(false);
  const isDefault = defaultFor(prefs, favoriteTabOf(model)) === model.id;
  const active = model.id === selectedModelId;

  return (
    <div className={`model-row-wrap ${highlight ? "just-added" : ""}`}>
      <div className={`model-row ${active ? "in-use" : ""}`}>
        <span className="provenance-dot local" aria-hidden="true" />
        <span className="model-row-name" title={model.name}>
          {model.name}
        </span>
        <span className="model-row-chips">
          {capabilityChips(model).map((c) => (
            <span key={c.label} className="cap-chip" title={c.title}>
              {c.label}
            </span>
          ))}
        </span>
        <span className="model-row-cost">Free · private · offline</span>
        <span className="model-row-actions">
          {isDefault ? (
            <span className="default-tag">Default</span>
          ) : (
            <button
              className="btn-text"
              onClick={() => makeDefault(model.id, model).then(() => onMadeDefault?.())}
            >
              Make default
            </button>
          )}
          {inStore && (
            <button className="btn-use" disabled={useDisabled} onClick={() => use(model.id)}>
              {useLabel ?? (active ? "In use" : "Use")}
            </button>
          )}
          <StarButton model={model} />
          <button
            className="btn-text"
            aria-expanded={open}
            aria-label={`Details for ${model.name}`}
            onClick={() => setOpen((o) => !o)}
          >
            Details {open ? "▴" : "▾"}
          </button>
        </span>
      </div>
      {open && (
        <div className="model-row-details">
          <span>{details}</span>
          <span className="model-row-path">{path}</span>
          <button className="btn-text danger" onClick={onDelete}>
            Delete from this PC
          </button>
        </div>
      )}
    </div>
  );
}

/** "Added X. Star it?" after a download finishes. */
function AddedOffer({ name, onStar, onDismiss }: { name: string; onStar: () => void; onDismiss: () => void }) {
  return (
    <p className="added-offer">
      Added {name}. Star it?{" "}
      <button
        className="btn-text"
        onClick={() => {
          onStar();
          onDismiss();
        }}
      >
        ★ Star
      </button>
      <button className="btn-text" onClick={onDismiss}>
        Not now
      </button>
    </p>
  );
}

/** Local chat models, in the same row style as every other group. */
function LocalChatGroup({
  entries,
  justAdded,
  onDismissAdded,
}: {
  entries: ModelEntry[];
  justAdded: string | null;
  onDismissAdded: () => void;
}) {
  const models = useAppStore((s) => s.models);
  const prefs = useAppStore((s) => s.modelPrefs);
  const loadingModel = useAppStore((s) => s.loadingModel);
  const refreshLibrary = useAppStore((s) => s.refreshLibrary);
  const toggle = useAppStore((s) => s.toggleFavoriteModel);
  const [removing, setRemoving] = useState<ModelEntry | null>(null);
  const totalMb = entries.reduce((s, m) => s + (m.size_bytes ?? 0), 0) / 1048576;
  const added = justAdded ? entries.find((e) => e.path.split(/[\\/]/).pop() === justAdded) : undefined;
  const addedModel = added ? models.find((x) => x.id === added.id) : undefined;

  return (
    <section className="model-section">
      <div className="section-head">
        <h2 className="section-title">
          <span className="provenance-dot local" aria-hidden="true" /> On this PC
        </h2>
        <span className="disk-total">{formatSize(Math.round(totalMb))} on disk</span>
      </div>
      {added && addedModel && !prefs.favorites.chat.includes(added.id) && (
        <AddedOffer name={added.name} onStar={() => toggle(addedModel.id, addedModel)} onDismiss={onDismissAdded} />
      )}
      <div className="model-rows">
        {entries.map((e) => {
          const m: Model = models.find((x) => x.id === e.id) ?? {
            id: e.id,
            name: e.name,
            provenance: "local",
            vision: e.vision,
          };
          const loading = loadingModel?.id === e.id;
          return (
            <LocalRow
              key={e.id}
              model={m}
              details={[e.quant, e.size_bytes ? formatSize(Math.round(e.size_bytes / 1048576)) : null]
                .filter(Boolean)
                .join(" · ")}
              path={e.path}
              inStore
              highlight={added?.id === e.id}
              useLabel={loading ? loadingModel?.label : undefined}
              useDisabled={loading}
              onDelete={() => setRemoving(e)}
            />
          );
        })}
      </div>
      {removing && (
        <ConfirmDialog
          title={`Delete ${removing.name}?`}
          body="Its file is removed from this PC. You can download it again later."
          onCancel={() => setRemoving(null)}
          onConfirm={async () => {
            const e = removing;
            setRemoving(null);
            await deleteModelEntry(e.id).catch(() => {});
            await refreshLibrary();
          }}
        />
      )}
    </section>
  );
}

/** Local diffusion files (`MOD-2`), as rows. Starring and "Make default" work
 * on any of them; making one default also makes it the active checkpoint. */
function LocalImageGroup({ lib }: { lib: ImageLibrary }) {
  const models = useAppStore((s) => s.models);
  const prefs = useAppStore((s) => s.modelPrefs);
  const toggle = useAppStore((s) => s.toggleFavoriteModel);
  const [removing, setRemoving] = useState<ImageModel | null>(null);
  const added = lib.justAdded ? lib.models.find((m) => m.name === lib.justAdded) : undefined;
  const totalBytes = lib.models.reduce((s, m) => s + m.size_bytes, 0);

  return (
    <section className="model-section">
      <div className="section-head">
        <h2 className="section-title">
          <span className="provenance-dot local" aria-hidden="true" /> On this PC
        </h2>
        <span className="disk-total">{formatBytes(totalBytes)} on disk</span>
      </div>
      {lib.status && !lib.status.engine_installed && (
        <p className="add-help">
          These need the image runtime to draw. Your next download sets it up, or install it under
          Runtime → Images.
        </p>
      )}
      {added && !prefs.favorites.media.includes(localImageAsModel(added).id) && (
        <AddedOffer
          name={added.name}
          onStar={() => {
            const m = localImageAsModel(added);
            toggle(m.id, m);
          }}
          onDismiss={() => lib.setJustAdded(null)}
        />
      )}
      <div className="model-rows">
        {lib.models.map((im) => {
          const m = localImageAsModel(im);
          return (
            <LocalRow
              key={im.path}
              model={m}
              details={formatBytes(im.size_bytes)}
              path={im.path}
              inStore={models.some((x) => x.id === m.id)}
              highlight={added?.path === im.path}
              onDelete={() => setRemoving(im)}
              onMadeDefault={() => lib.refresh()}
            />
          );
        })}
      </div>
      {removing && (
        <ConfirmDialog
          title={`Delete ${removing.name}?`}
          body="Its file is removed from this PC. You can download it again later."
          onCancel={() => setRemoving(null)}
          onConfirm={async () => {
            const im = removing;
            setRemoving(null);
            await lib.remove(im);
          }}
        />
      )}
    </section>
  );
}

/** A provider's models as compact rows; there can be hundreds. */
function CloudGroup({ label, models, expandAll }: { label: string; models: Model[]; expandAll: boolean }) {
  const [all, setAll] = useState(false);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const favs = useAppStore((s) => s.modelPrefs.favorites);
  // Starred models float to the top of their group.
  const sorted = [...models].sort(
    (a, b) =>
      Number(favs[favoriteTabOf(b)].includes(b.id)) - Number(favs[favoriteTabOf(a)].includes(a.id))
  );
  const found = sorted.filter((m) => matches(m, q));
  const visible = all || expandAll || q ? found : found.slice(0, GROUP_PREVIEW);
  return (
    <section className="model-section">
      <div className="section-head">
        <h2 className="section-title">
          <span className="provenance-dot cloud" aria-hidden="true" /> Via {label}
        </h2>
        <span className="group-tools">
          <span className="disk-total">
            {q ? `${found.length} of ${models.length}` : models.length}{" "}
            {models.length === 1 ? "model" : "models"}
          </span>
          {models.length > GROUP_PREVIEW && (
            <input
              className="models-search"
              type="search"
              placeholder={`Search ${label}`}
              aria-label={`Search ${label} models`}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          )}
        </span>
      </div>
      <div className="model-rows">
        {visible.map((m) => (
          <ModelRow key={m.id} model={m} />
        ))}
      </div>
      {q && found.length === 0 && (
        <p className="add-help group-empty">
          No {label} model matches "{query.trim()}".
        </p>
      )}
      {visible.length < found.length && (
        <button className="link-button" onClick={() => setAll(true)}>
          … show all {found.length}
        </button>
      )}
    </section>
  );
}

/** `MOD-8`: local and cloud side by side as equal first steps. */
function FirstRun({
  catalog,
  onPickLocal,
  onConnect,
  onAdded,
  setError,
}: {
  catalog: CatalogEntry[];
  onPickLocal: () => void;
  onConnect: () => void;
  onAdded: (filename: string) => void;
  setError: (e: string | null) => void;
}) {
  const progress = useAppStore((s) => s.modelDownloads);
  const downloadCatalogModel = useAppStore((s) => s.downloadCatalogModel);
  const use = useUseModel();
  const firstPick = catalog.find((e) => e.fit !== "wont-fit") ?? catalog[0];

  // First-run: detect → recommend → one click → into chat (5.4.1).
  async function getStarted(entry: CatalogEntry) {
    try {
      await downloadCatalogModel(entry);
      const fresh = await listModels();
      const m = fresh.find((x) => x.path.split(/[\\/]/).pop() === filenameOf(entry.url));
      onAdded(filenameOf(entry.url));
      if (m) use(m.id);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <section className="first-run">
      <div className="first-run-option">
        <h2 className="first-run-title">Get started in one step</h2>
        {firstPick ? (
          <>
            <p className="first-run-body">
              For this PC we recommend <strong>{firstPick.name}</strong> ({formatSize(firstPick.size_mb)}
              ). {FIT_LABEL[firstPick.fit]}. Free, private, and it works offline.
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
          </>
        ) : (
          <button className="btn-primary big" onClick={onPickLocal}>
            See models for this PC
          </button>
        )}
      </div>
      <div className="first-run-option">
        <h2 className="first-run-title">Use an account you have</h2>
        <p className="first-run-body">
          Already have an OpenAI, Anthropic or OpenRouter account? Connect it and chat with its
          models right away.
        </p>
        <button className="btn-secondary big" onClick={onConnect}>
          Connect it →
        </button>
      </div>
    </section>
  );
}

/** Door 1 on the Chat tab: the curated GGUF catalog with fit badges. */
function RecommendedChat({
  catalog,
  onAdded,
  setError,
}: {
  catalog: CatalogEntry[];
  onAdded: (filename: string) => void;
  setError: (e: string | null) => void;
}) {
  const libraryModels = useAppStore((s) => s.libraryModels);
  const progress = useAppStore((s) => s.modelDownloads);
  const downloadCatalogModel = useAppStore((s) => s.downloadCatalogModel);
  const have = new Set(libraryModels.map((m) => m.path.split(/[\\/]/).pop()));

  async function download(entry: CatalogEntry) {
    try {
      await downloadCatalogModel(entry);
      onAdded(filenameOf(entry.url));
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="model-rows catalog-list">
      {catalog.map((entry) => {
        const prog = progress[entry.id];
        const state: CatalogState = have.has(filenameOf(entry.url))
          ? "owned"
          : prog === undefined
            ? "idle"
            : prog;
        return (
          <CatalogRow
            key={entry.id}
            name={entry.name}
            description={entry.description}
            chips={entry.vision ? ["Sees images"] : []}
            fit={entry.fit}
            fitLabel={FIT_LABEL[entry.fit]}
            size={formatSize(entry.size_mb)}
            details={[entry.quant, entry.speed, entry.license ?? ""]}
            state={state}
            onDownload={() => download(entry)}
          />
        );
      })}
    </div>
  );
}

/** Door 2 on the Chat tab (`MKT-6`, `MOD-7`): one field for a Hugging Face
 * repo or URL, a GitHub repo, or a direct .gguf link. It says what it got
 * before fetching anything; a repo opens the size slider. */
function LinkChat({
  onAdded,
  setError,
}: {
  onAdded: (filename: string) => void;
  setError: (e: string | null) => void;
}) {
  const [input, setInput] = useState("");
  const [finding, setFinding] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [groups, setGroups] = useState<RepoGroup[]>([]);
  const libraryModels = useAppStore((s) => s.libraryModels);
  const progress = useAppStore((s) => s.modelDownloads);
  const downloadCatalogModel = useAppStore((s) => s.downloadCatalogModel);
  const have = new Set(libraryModels.map((m) => m.path.split(/[\\/]/).pop()));
  const kind = classifyModelLink(input, "chat");

  async function download(entry: CatalogEntry) {
    try {
      await downloadCatalogModel(entry);
      onAdded(filenameOf(entry.url));
    } catch (e) {
      setError(String(e));
    }
  }

  async function go() {
    setProblem(null);
    if (kind.kind === "empty") return;
    if (kind.kind === "invalid") {
      setProblem(kind.label);
      return;
    }
    if (kind.kind === "file") {
      // A direct link skips straight to download.
      setInput("");
      await download({
        id: `url:${kind.url}`,
        name: kind.filename.replace(/\.gguf$/i, ""),
        description: "",
        quant: "",
        size_mb: 0,
        vision: false,
        url: kind.url,
        source: "url",
        license: null,
        fit: "great",
        speed: "",
      });
      return;
    }
    setFinding(true);
    try {
      const files = kind.kind === "github" ? await listGithubModels(kind.repo) : await listRepoFiles(kind.repo);
      if (files.length === 0) {
        setProblem(
          kind.kind === "github" ? `No GGUF files in the releases of ${kind.repo}.` : noGgufMessage(kind.repo)
        );
        return;
      }
      const sorted = [...files].sort((a, b) => a.size_mb - b.size_mb);
      // Start on the largest size that still runs well here.
      const fits = sorted.map((f) => f.fit !== "wont-fit");
      const pick = Math.max(0, fits.lastIndexOf(true));
      setGroups((g) => [{ repo: kind.repo, files: sorted, pick }, ...g.filter((x) => x.repo !== kind.repo)]);
      setInput("");
    } catch (e) {
      const msg = String(e);
      setProblem(
        /404|not found/i.test(msg)
          ? `Couldn't find ${kind.repo}. Check the spelling, or that the repo is public.`
          : `Couldn't read ${kind.repo}: ${msg}`
      );
    } finally {
      setFinding(false);
    }
  }

  return (
    <div className="add-door-body">
      <div className="add-row">
        <input
          className="add-input"
          aria-label="Hugging Face repo, GitHub repo or .gguf link"
          placeholder="bartowski/Qwen2.5-7B-Instruct-GGUF  or  https://…/model.gguf"
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            setProblem(null);
          }}
          onKeyDown={(e) => e.key === "Enter" && go()}
        />
        <button
          className="btn-primary"
          onClick={go}
          disabled={finding || kind.kind === "empty" || kind.kind === "invalid"}
        >
          {finding ? "Finding…" : kind.kind === "file" ? "Download" : "Find files"}
        </button>
      </div>
      {problem ? (
        <p className="link-kind bad" role="alert">
          {problem}
        </p>
      ) : (
        kind.kind !== "empty" && <p className={`link-kind ${kind.kind === "invalid" ? "bad" : ""}`}>{kind.label}</p>
      )}

      {groups.map((g) => {
        const file = g.files[g.pick];
        const prog = progress[file.id];
        const owned = have.has(filenameOf(file.url));
        return (
          <div className="quant-card" key={g.repo}>
            <div className="quant-repo">
              {g.repo} · {g.files.length} {g.files.length === 1 ? "file" : "files"}
            </div>
            <input
              type="range"
              className="quant-slider"
              min={0}
              max={g.files.length - 1}
              value={g.pick}
              aria-label="Choose a size"
              onChange={(e) =>
                setGroups((all) => all.map((x) => (x.repo === g.repo ? { ...x, pick: Number(e.target.value) } : x)))
              }
            />
            <div className="quant-scale">
              <span>Smaller · faster</span>
              <span>Larger · better answers</span>
            </div>
            <div className="quant-detail">
              <span className={`fit-badge fit-${file.fit}`}>{FIT_LABEL[file.fit]}</span>
              <span className="model-size">{formatSize(file.size_mb)}</span>
              <details className="model-details inline">
                <summary>Details</summary>
                <span className="model-speed">
                  {[file.quant || "GGUF", file.speed].filter(Boolean).join(" · ")}
                </span>
              </details>
            </div>
            <div className="model-card-actions">
              {owned ? (
                <span className="owned-note">On this PC</span>
              ) : prog === undefined ? (
                <button className="btn-download" disabled={file.fit === "wont-fit"} onClick={() => download(file)}>
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
  );
}
