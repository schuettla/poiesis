import { useEffect, useRef, useState } from "react";
import { useAppStore, useSelectedModel } from "../../lib/store";
import type { Model } from "../../lib/types";
import "./ModelPicker.css";

function Dot({ provenance }: { provenance: Model["provenance"] }) {
  return <span className={`provenance-dot ${provenance}`} aria-hidden="true" />;
}

const CLOUD_LIMIT = 60;

export default function ModelPicker() {
  const models = useAppStore((s) => s.models);
  const selected = useSelectedModel();
  const selectModel = useAppStore((s) => s.selectModel);
  const filter = useAppStore((s) => s.modelFilter);
  const setFilter = useAppStore((s) => s.setModelFilter);
  const setView = useAppStore((s) => s.setView);

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

  const localModels = models.filter((m) => m.provenance === "local");
  const allCloud = models.filter((m) => m.provenance === "cloud");
  const localOnly = filter === "local";

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

  function goToSettings() {
    setView("settings");
    setOpen(false);
  }

  return (
    <div className="model-picker" ref={ref}>
      <button
        className="model-picker-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <Dot provenance={selected.provenance} />
        <span>{selected.name}</span>
        <span className="caret" aria-hidden="true">
          ▾
        </span>
      </button>

      {open && (
        <div className="model-dropdown open" role="listbox" aria-label="Choose a model">
          <div className="filter-row">
            <button
              className={`filter-chip ${!localOnly ? "active" : ""}`}
              aria-pressed={!localOnly}
              onClick={() => setFilter("all")}
            >
              All models
            </button>
            <button
              className={`filter-chip ${localOnly ? "active" : ""}`}
              aria-pressed={localOnly}
              onClick={() => setFilter("local")}
            >
              Local only
            </button>
          </div>

          <div className="model-group-label">On this device</div>
          {localModels.map((m) => (
            <ModelRow key={m.id} model={m} selected={m.id === selected.id} onClick={() => choose(m)} />
          ))}

          {!localOnly && (
            <>
              <div className="model-group-label">Cloud · your key</div>
              {allCloud.length > 8 && (
                <input
                  className="cloud-search"
                  placeholder="Filter cloud models…"
                  value={cloudQuery}
                  onChange={(e) => setCloudQuery(e.target.value)}
                />
              )}
              {allCloud.length === 0 ? (
                <div className="add-key-row">
                  <a href="#" onClick={(e) => (e.preventDefault(), goToSettings())}>
                    + Add a provider key
                  </a>{" "}
                  to use cloud models with your own key
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
    </div>
  );
}
