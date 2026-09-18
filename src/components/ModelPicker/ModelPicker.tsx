import { useEffect, useRef, useState } from "react";
import { useAppStore, useSelectedModel } from "../../lib/store";
import type { Model } from "../../lib/types";
import { availableFavorites, isMediaModel } from "../../lib/modelPrefs";
import { StarButton } from "../Models/ModelRows";
import "./ModelPicker.css";

function Dot({ provenance }: { provenance: Model["provenance"] }) {
  return <span className={`provenance-dot ${provenance}`} aria-hidden="true" />;
}

const CLOUD_LIMIT = 60;

/**
 * `compact` shrinks the trigger for its home in the composer's footer row;
 * `dropUp` opens the list above it, since there is nothing but the window edge
 * below.
 */
export default function ModelPicker({
  compact = false,
  dropUp = false,
}: {
  compact?: boolean;
  dropUp?: boolean;
} = {}) {
  const models = useAppStore((s) => s.models);
  const selected = useSelectedModel();
  const selectModel = useAppStore((s) => s.selectModel);
  const filter = useAppStore((s) => s.modelFilter);
  const setFilter = useAppStore((s) => s.setModelFilter);
  const openProviders = useAppStore((s) => s.openProviders);
  const openRuntime = useAppStore((s) => s.openRuntime);
  const prefs = useAppStore((s) => s.modelPrefs);

  const [open, setOpen] = useState(false);
  const [cloudQuery, setCloudQuery] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const isMedia = isMediaModel;
  const localOnly = filter === "local";
  const cloudOnly = filter === "cloud";
  // `MOD-4`: favorites lead, in the user's order, and are never cut by
  // `CLOUD_LIMIT`. They are shown once, here, and not repeated below.
  const favorites = [
    ...availableFavorites(models, prefs, "chat"),
    ...availableFavorites(models, prefs, "media"),
  ].filter((m) => (localOnly ? m.provenance !== "cloud" : cloudOnly ? m.provenance === "cloud" : true));
  const favIds = new Set(favorites.map((m) => m.id));
  const notFav = (m: Model) => !favIds.has(m.id);
  const localModels = models.filter((m) => m.provenance === "local" && !isMedia(m) && notFav(m));
  // A user's own connected server (Ollama, LM Studio, ...) never leaves the
  // machine, so it counts as "local" for the filter even though it's routed
  // like a cloud model.
  const endpointModels = models.filter((m) => m.provenance === "endpoint" && !isMedia(m) && notFav(m));
  const allCloud = models.filter((m) => m.provenance === "cloud" && !isMedia(m) && notFav(m));
  const anyCloud = models.some((m) => m.provenance === "cloud" && !isMedia(m));
  // `PIK-1`: media models get their own group below chat models, not folded
  // into "On this device" / "Cloud" — picking one changes what *sending*
  // does, which chat models never do, so they read differently on purpose.
  const mediaModels = models.filter((m) => isMedia(m) && notFav(m));
  const visibleMedia = localOnly
    ? mediaModels.filter((m) => m.provenance === "local")
    : cloudOnly
      ? mediaModels.filter((m) => m.provenance === "cloud")
      : mediaModels;

  const q = cloudQuery.trim().toLowerCase();
  const filteredCloud = q
    ? allCloud.filter((m) => m.name.toLowerCase().includes(q) || (m.meta ?? "").toLowerCase().includes(q))
    : allCloud;
  const cloudModels = filteredCloud.slice(0, CLOUD_LIMIT);
  const cloudHidden = filteredCloud.length - cloudModels.length;

  function choose(m: Model) {
    selectModel(m.id);
    setOpen(false);
  }

  // `PRV-6`: keys live on Providers, own servers on Runtime → Your servers.
  function goToProviders() {
    openProviders();
    setOpen(false);
  }
  function goToServers() {
    openRuntime("servers");
    setOpen(false);
  }

  return (
    <div
      className={`model-picker ${compact ? "compact" : ""} ${dropUp ? "up" : ""}`}
      ref={ref}
    >
      <button
        className="model-picker-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Model: ${selected.name}`}
        title={`Model: ${selected.name}`}
        onClick={() => setOpen((o) => !o)}
      >
        <Dot provenance={selected.provenance} />
        <span className="model-picker-name">{selected.name}</span>
        <span className="caret" aria-hidden="true">
          {dropUp ? "▴" : "▾"}
        </span>
      </button>

      {open && (
        <div className="model-dropdown open" role="listbox" aria-label="Choose a model">
          <div className="filter-row">
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
          </div>

          {favorites.length > 0 && (
            <>
              <div className="model-group-label">Favorites</div>
              {favorites.map((m) => (
                <ModelRow key={m.id} model={m} selected={m.id === selected.id} onClick={() => choose(m)} />
              ))}
            </>
          )}

          {!cloudOnly && localModels.length > 0 && (
            <>
              <div className="model-group-label">On this PC</div>
              {localModels.map((m) => (
                <ModelRow key={m.id} model={m} selected={m.id === selected.id} onClick={() => choose(m)} />
              ))}
            </>
          )}

          {/* A user's own connected server — shown under both filters, since
              it runs on their machine like the row above. Omitted entirely
              when nothing is connected, so a fresh install looks unchanged. */}
          {!cloudOnly && endpointModels.length > 0 && (
            <>
              <div className="model-group-label">Your servers</div>
              {endpointModels.map((m) => (
                <ModelRow key={m.id} model={m} selected={m.id === selected.id} onClick={() => choose(m)} />
              ))}
            </>
          )}

          {!localOnly && (
            <>
              <div className="model-group-label">Cloud · your accounts</div>
              {allCloud.length > 8 && (
                <input
                  className="cloud-search"
                  placeholder="Filter cloud models…"
                  value={cloudQuery}
                  onChange={(e) => setCloudQuery(e.target.value)}
                />
              )}
              {!anyCloud ? (
                <div className="add-key-row">
                  <a href="#" onClick={(e) => (e.preventDefault(), goToProviders())}>
                    + Connect an account
                  </a>{" "}
                  to use cloud models with your own key, or{" "}
                  <a href="#" onClick={(e) => (e.preventDefault(), goToServers())}>
                    + connect a local server
                  </a>{" "}
                  like Ollama or LM Studio
                </div>
              ) : (
                <>
                  {cloudModels.map((m) => (
                    <ModelRow
                      key={m.id}
                      model={m}
                      selected={m.id === selected.id}
                      onClick={() => choose(m)}
                    />
                  ))}
                  {cloudHidden > 0 && (
                    <div className="model-group-label">+{cloudHidden} more — refine the filter</div>
                  )}
                </>
              )}
            </>
          )}

          {/* `PIK-1`: omitted entirely when empty, so a fresh install with no
              engine and no key sees today's picker unchanged. */}
          {visibleMedia.length > 0 && (
            <>
              <div className="model-group-label">Images &amp; video</div>
              {visibleMedia.map((m) => (
                <ModelRow key={m.id} model={m} selected={m.id === selected.id} onClick={() => choose(m)} />
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}

function ModelRow({
  model,
  selected,
  onClick,
}: {
  model: Model;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <div
      className={`model-option ${selected ? "selected" : ""}`}
      role="option"
      aria-selected={selected}
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <Dot provenance={model.provenance} />
      <span className="name">{model.name}</span>
      {model.meta && <span className="meta">{model.meta}</span>}
      {model.tools === false && (
        <span
          className="meta no-tools"
          title="This model can't use tools — it can chat, but it can't search, read files, or run skills."
        >
          chat only
        </span>
      )}
      {model.priceLabel && <span className="price">{model.priceLabel}</span>}
      {model.modality && model.modality !== "chat" && !model.priceLabel && (
        <span className="price">free</span>
      )}
      <StarButton model={model} />
    </div>
  );
}
