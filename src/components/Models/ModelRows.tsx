import { useState } from "react";
import { useAppStore } from "../../lib/store";
import type { Model } from "../../lib/types";
import { defaultFor, favoriteTabOf, type FavoriteTab } from "../../lib/modelPrefs";

const PROVIDER_NAMES: Record<string, string> = {
  openai: "OpenAI",
  openrouter: "OpenRouter",
  anthropic: "Anthropic",
};

/** Where a model runs, in the words every screen uses (`Vocabulary`). */
export function whereItRuns(m: Model): string {
  if (m.provenance === "local") return "On this PC";
  if (m.provenance === "endpoint") return `Your server · ${m.endpointLabel ?? m.meta ?? "server"}`;
  const provider = m.backendLabel ?? PROVIDER_NAMES[m.provider ?? ""] ?? m.meta ?? "the cloud";
  return `Via ${provider}`;
}

function money(v: number): string {
  if (v === 0) return "0";
  return v < 0.1 ? v.toFixed(3).replace(/0+$/, "") : v < 10 ? v.toFixed(2).replace(/\.?0+$/, "") : v.toFixed(0);
}

/** What using it costs, in plain words (`MOD-5`). */
export function costLabel(m: Model): string | null {
  if (m.provenance === "local" || m.provenance === "endpoint") return "Free · private · offline";
  if (m.priceLabel) return m.priceLabel;
  if (m.promptPerMtok !== undefined && m.outputPerMtok !== undefined) {
    if (m.promptPerMtok === 0 && m.outputPerMtok === 0) return "Free tier";
    return `$${money(m.promptPerMtok)} / $${money(m.outputPerMtok)} per 1M`;
  }
  return null;
}

/** Up to two capability chips. */
export function capabilityChips(m: Model): { label: string; title?: string }[] {
  const chips: { label: string; title?: string }[] = [];
  if (m.modality === "image") chips.push({ label: "Image" });
  if (m.modality === "video") chips.push({ label: "Video" });
  if (m.vision) chips.push({ label: "Sees images" });
  if (m.tools === false) {
    chips.push({
      label: "Chat only",
      title: "This model can't use tools — it can chat, but it can't search, read files, or run skills.",
    });
  }
  return chips.slice(0, 2);
}

/** Why a favorite can't be used right now, from its id alone. */
export function unavailableReason(id: string): string {
  if (id.startsWith("cloud:")) {
    const provider = PROVIDER_NAMES[id.split(":")[1]] ?? "its provider";
    return `Connect ${provider} to use it`;
  }
  if (id.startsWith("endpoint:")) return "Its server is off or out of reach";
  if (id.startsWith("media:local/")) return "The file isn't on this PC any more";
  if (id.startsWith("media:")) return "Its provider isn't connected";
  return "Not on this PC any more";
}

/** ★ toggle. The default can't be unstarred, so its star says why instead. */
export function StarButton({ model }: { model: Model }) {
  const favorites = useAppStore((s) => s.modelPrefs.favorites);
  const prefs = useAppStore((s) => s.modelPrefs);
  const toggle = useAppStore((s) => s.toggleFavoriteModel);
  const tab = favoriteTabOf(model);
  const starred = favorites[tab].includes(model.id);
  const isDefault = defaultFor(prefs, tab) === model.id;
  const title = isDefault
    ? "Pick another default first."
    : starred
      ? "Remove from favorites"
      : "Add to favorites";
  return (
    <button
      className={`star-btn ${starred ? "on" : ""}`}
      aria-pressed={starred}
      aria-label={`${title}: ${model.name}`}
      title={title}
      disabled={isDefault}
      onClick={(e) => {
        e.stopPropagation();
        toggle(model.id, model);
      }}
    >
      {starred ? "★" : "☆"}
    </button>
  );
}

/** Pick a model and go back to chatting with it, as "Use" always did. */
export function useUseModel() {
  const selectModel = useAppStore((s) => s.selectModel);
  const setView = useAppStore((s) => s.setView);
  return (id: string) => {
    selectModel(id);
    setView("chat");
  };
}

/** One compact row, for the long lists (servers, cloud, media). */
export function ModelRow({ model }: { model: Model }) {
  const prefs = useAppStore((s) => s.modelPrefs);
  const selectedModelId = useAppStore((s) => s.selectedModelId);
  const makeDefault = useAppStore((s) => s.setDefaultModelPref);
  const use = useUseModel();
  const [copied, setCopied] = useState(false);
  const tab = favoriteTabOf(model);
  const isDefault = defaultFor(prefs, tab) === model.id;
  const cost = costLabel(model);

  return (
    <div className={`model-row ${model.id === selectedModelId ? "in-use" : ""}`}>
      <span className={`provenance-dot ${model.provenance}`} aria-hidden="true" />
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
      {cost && <span className="model-row-cost">{cost}</span>}
      <span className="model-row-actions">
        {isDefault ? (
          <span className="default-tag">Default</span>
        ) : (
          model.modality !== "video" && (
            <button className="btn-text" onClick={() => makeDefault(model.id, model)}>
              Make default
            </button>
          )
        )}
        <button className="btn-use" onClick={() => use(model.id)}>
          {model.id === selectedModelId ? "In use" : "Use"}
        </button>
        <StarButton model={model} />
        {model.cloudModel && (
          <button
            className="btn-text"
            title="Copy model id"
            aria-label={`Copy the model id of ${model.name}`}
            onClick={() => {
              navigator.clipboard?.writeText(model.cloudModel ?? model.id).catch(() => {});
              setCopied(true);
              setTimeout(() => setCopied(false), 1400);
            }}
          >
            {copied ? "Copied" : "⧉"}
          </button>
        )}
      </span>
    </div>
  );
}

/** Where a catalog download stands: on disk already, not started, a percent,
 * or just finished. */
export type CatalogState = "owned" | "idle" | "done" | number;

/** One model you could download (`MOD-7` door 1), in the page's row style:
 * what it is and whether it fits this PC up front, the technical bits under
 * Details. */
export function CatalogRow({
  name,
  description,
  chips = [],
  fit,
  fitLabel,
  size,
  details,
  state,
  onDownload,
}: {
  name: string;
  description: string;
  chips?: string[];
  fit: string;
  fitLabel: string;
  size: string;
  details: string[];
  state: CatalogState;
  onDownload: () => void;
}) {
  const [open, setOpen] = useState(false);
  const detailLine = details.filter(Boolean).join(" · ");
  return (
    <div className={`model-row-wrap catalog-row ${fit === "wont-fit" ? "wont-fit" : ""}`}>
      <div className="model-row">
        <span className="catalog-text">
          <span className="catalog-name-line">
            <span className="model-row-name">{name}</span>
            {chips.map((c) => (
              <span key={c} className="cap-chip">
                {c}
              </span>
            ))}
          </span>
          {description && <span className="catalog-desc">{description}</span>}
        </span>
        <span className={`fit-badge fit-${fit}`}>{fitLabel}</span>
        <span className="model-row-cost catalog-size">{size}</span>
        <span className="model-row-actions">
          {detailLine && (
            <button
              className="btn-text"
              aria-expanded={open}
              aria-label={`Details for ${name}`}
              onClick={() => setOpen((o) => !o)}
            >
              Details {open ? "▴" : "▾"}
            </button>
          )}
          <span className="catalog-action">
            {state === "owned" ? (
              <span className="owned-note">On this PC</span>
            ) : state === "done" ? (
              <span className="owned-note">Downloaded</span>
            ) : state === "idle" ? (
              <button
                className="btn-use"
                disabled={fit === "wont-fit"}
                title={fit === "wont-fit" ? "Too big for this PC" : undefined}
                onClick={onDownload}
              >
                Download
              </button>
            ) : (
              <span className="dl-progress" role="progressbar" aria-valuenow={state} aria-label={`Downloading ${name}`}>
                <span className="dl-bar" style={{ width: `${state}%` }} />
                <span className="dl-pct">{state}%</span>
              </span>
            )}
          </span>
        </span>
      </div>
      {open && detailLine && (
        <div className="model-row-details">
          <span>{detailLine}</span>
        </div>
      )}
    </div>
  );
}

/** `MOD-4`: the Favorites section, first on each tab. Reorder by dragging the
 * handle, or with ↑/↓ while a row has focus. */
export function FavoritesSection({ tab, models }: { tab: FavoriteTab; models: Model[] }) {
  const prefs = useAppStore((s) => s.modelPrefs);
  const move = useAppStore((s) => s.moveFavoriteModel);
  const reorder = useAppStore((s) => s.reorderFavoriteModels);
  const toggle = useAppStore((s) => s.toggleFavoriteModel);
  const makeDefault = useAppStore((s) => s.setDefaultModelPref);
  const use = useUseModel();
  const [dragId, setDragId] = useState<string | null>(null);

  const ids = prefs.favorites[tab];
  const byId = new Map(models.map((m) => [m.id, m]));
  const def = defaultFor(prefs, tab);

  return (
    <section className="model-section favorites">
      <div className="section-head">
        <h2 className="section-title">Favorites</h2>
        {ids.length > 1 && <span className="section-note">Drag to reorder · these lead the model picker</span>}
      </div>
      {ids.length === 0 ? (
        <p className="add-help">
          Star models you use often. They'll lead the model picker, and I'll fall back to them in
          order if your default isn't available.
        </p>
      ) : (
        <ol className="fav-list">
          {ids.map((id, i) => {
            const m = byId.get(id);
            const isDefault = id === def;
            const pinned = isDefault && i === 0;
            return (
              <li
                key={id}
                className={`fav-row ${m ? "" : "unavailable"} ${dragId === id ? "dragging" : ""}`}
                tabIndex={0}
                draggable={!pinned}
                aria-label={`${i + 1}. ${m?.name ?? prefs.labels[id] ?? id}`}
                onDragStart={() => setDragId(id)}
                onDragEnd={() => setDragId(null)}
                onDragOver={(e) => e.preventDefault()}
                onDrop={(e) => {
                  e.preventDefault();
                  if (!dragId || dragId === id) return;
                  const next = ids.filter((x) => x !== dragId);
                  next.splice(next.indexOf(id) + (ids.indexOf(dragId) < i ? 1 : 0), 0, dragId);
                  reorder(tab, next);
                  setDragId(null);
                }}
                onKeyDown={(e) => {
                  if (e.target !== e.currentTarget) return;
                  if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                    e.preventDefault();
                    move(tab, id, e.key === "ArrowUp" ? -1 : 1);
                  }
                }}
              >
                <span className={`drag-handle ${pinned ? "pinned" : ""}`} aria-hidden="true">
                  {pinned ? "" : "⠿"}
                </span>
                <span className="fav-pos">{i + 1}</span>
                <span className={`provenance-dot ${m?.provenance ?? "none"}`} aria-hidden="true" />
                <span className="model-row-name">{m?.name ?? prefs.labels[id] ?? id}</span>
                <span className="fav-where">{m ? whereItRuns(m) : `Not available · ${unavailableReason(id)}`}</span>
                <span className="model-row-actions">
                  {isDefault ? (
                    <span className="default-tag">Default</span>
                  ) : (
                    m &&
                    m.modality !== "video" && (
                      <button className="btn-text" onClick={() => makeDefault(id, m)}>
                        Make default
                      </button>
                    )
                  )}
                  {m && (
                    <button className="btn-use" onClick={() => use(id)}>
                      Use
                    </button>
                  )}
                  <button
                    className="star-btn on"
                    aria-label={isDefault ? "Pick another default first." : `Remove ${m?.name ?? id} from favorites`}
                    title={isDefault ? "Pick another default first." : "Remove from favorites"}
                    disabled={isDefault}
                    onClick={() => toggle(id, m)}
                  >
                    ★
                  </button>
                </span>
              </li>
            );
          })}
        </ol>
      )}
      {ids.length > 0 && (
        <p className="section-note">
          {tab === "chat"
            ? "If your default isn't available, I'll use the next favorite that is."
            : "If your default image model isn't available, I'll use the next image favorite that is."}
        </p>
      )}
    </section>
  );
}
